// Derived from herdr src/client/clipboard_images.rs and the
// `clipboard_image_target` block of src/client/shell/input.rs
// (Apache-2.0).
//! The R9 clipboard-image bridge: on remote-capable endpoints the
//! configured paste key, an empty bracketed paste, or a dropped image
//! path uploads the local image through `terminal.put` and pastes the
//! returned path with the normal pane paste.

use super::*;

/// The terminal an image upload targets — the reference's
/// `ClientClipboardImageTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImagePasteTarget {
    Pane(String),
    Popup(String),
}

impl ImagePasteTarget {
    fn terminal(&self) -> &str {
        match self {
            Self::Pane(terminal) | Self::Popup(terminal) => terminal,
        }
    }
}

impl App {
    /// The client-side facts of `endpoint_accepts_local_images`: the
    /// clipboard lives on this machine while the program may not.
    /// `remote` is `--remote`; `over_ssh` is this client itself running
    /// inside an SSH session. The third arm — a `machine/tN` focused
    /// terminal — is checked per target.
    pub fn set_image_paste_context(&mut self, remote: bool, over_ssh: bool) {
        self.image_paste_remote = remote;
        self.image_paste_over_ssh = over_ssh;
    }

    /// `endpoint_accepts_local_images` for the resolved target.
    fn image_bridge_applies(&self, terminal: &str) -> bool {
        self.image_paste_remote || self.image_paste_over_ssh || terminal.contains('/')
    }

    /// The reference `clipboard_image_target`: the popup terminal while
    /// it is open, else the focused pane in Normal mode with no overlay
    /// and no armed prefix. A peek observes only and has no target.
    pub(super) fn clipboard_image_target(&self) -> Option<ImagePasteTarget> {
        if let Some(popup) = &self.popup {
            return Some(ImagePasteTarget::Popup(popup.terminal.clone()));
        }
        if self.popup_pending.is_some()
            || self.peek.is_some()
            || self.dialog.is_some()
            || self.menu.is_some()
            || self.navigator.is_some()
            || self.settings.is_some()
            || self.help
            || self.mode != Mode::Normal
            || self.prefix.armed()
        {
            return None;
        }
        self.focused.clone().map(ImagePasteTarget::Pane)
    }

    /// Does this key press match `keys.remote_image_paste`? An empty
    /// configured value disables the trigger.
    fn image_paste_trigger_matches(&self, key: &input::TerminalKey) -> bool {
        self.shell.remote_image_paste_key.is_some_and(|combo| {
            crate::config::terminal_key_matches_combo(&crate::input::terminal_key_of(key), combo)
        })
    }

    /// The press/repeat arm of `route_key_press`: a configured-trigger
    /// press starts an upload and is consumed; a re-dispatched repeat of
    /// a consumed press is swallowed. `None` means the key continues
    /// into normal routing — the program sees it unchanged.
    pub(super) fn route_image_paste_key(&mut self, key: &input::TerminalKey) -> Option<()> {
        if !self.image_paste_trigger_matches(key) {
            return None;
        }
        match key.input.action {
            hc_vt::Action::Repeat => Some(()),
            hc_vt::Action::Press if self.start_clipboard_image_paste() => {
                // Only a release-reporting host can ever send the
                // release — legacy hosts must not grow the set.
                if self.host_reports_key_releases {
                    self.image_paste_suppress.insert(key.identity());
                }
                Some(())
            }
            _ => None,
        }
    }

    /// The key/empty-paste trigger: read the clipboard for the resolved
    /// target. `true` consumed the gesture (upload started or a notice
    /// answered); `false` leaves the original input alone.
    fn start_clipboard_image_paste(&mut self) -> bool {
        let Some(target) = self.clipboard_image_target() else {
            return false;
        };
        if !self.image_bridge_applies(target.terminal()) {
            return false;
        }
        let Some(image) = (self.image_paste_reader)() else {
            return false;
        };
        self.submit_image_upload(target, image);
        true
    }

    /// The paste-arm of `UiEvent::Paste`: an empty bracketed paste is
    /// the terminal's image-only Ctrl+Shift+V, and a single absolute
    /// image path is a file drop. `true` consumed the paste.
    pub(super) fn try_image_paste_bridge(&mut self, text: &[u8]) -> bool {
        let Some(target) = self.clipboard_image_target() else {
            return false;
        };
        if !self.image_bridge_applies(target.terminal()) {
            return false;
        }
        let image = if text.is_empty() {
            (self.image_paste_reader)()
        } else {
            let Ok(text) = std::str::from_utf8(text) else {
                return false;
            };
            let Some((path, extension)) = crate::clipdrop::image_path_from_paste(text) else {
                return false;
            };
            crate::clipdrop::read_image_file(&path, extension)
        };
        let Some(image) = image else {
            return false;
        };
        self.submit_image_upload(target, image);
        true
    }

    /// `terminal.put` through the operations store, off the UI path.
    /// Over the core's 8 MB bound the key is consumed with a notice —
    /// like the reference's oversized-payload drop.
    fn submit_image_upload(
        &mut self,
        target: ImagePasteTarget,
        image: crate::clipboard::ClipboardImage,
    ) {
        if image.bytes.len() > crate::clipboard::MAX_CLIPBOARD_IMAGE_BYTES {
            self.show_notice(
                "Image too large",
                "The clipboard image exceeds the core's 8 MB upload limit",
            );
            return;
        }
        let terminal = target.terminal().to_owned();
        let Some(created_ms) = self.view.inventory.get(&terminal).map(|t| t.created_ms) else {
            self.show_notice(
                "Image paste unavailable",
                "The terminal is no longer running",
            );
            return;
        };
        let name = format!(
            "clipboard-{}.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            image.extension
        );
        self.submit_action(crate::operations::Action::PutImage {
            terminal,
            created_ms,
            name,
            data: crate::selection::base64_encode(&image.bytes),
        });
    }

    /// The `terminal.put` result: paste the written path through the
    /// normal pane paste — bracketed only when the program asked —
    /// unless the target moved while the upload ran, in which case the
    /// path surfaces as a notice instead of typing into the wrong pane.
    pub(super) fn on_image_put(&mut self, terminal: &str, path: &str) {
        match self.clipboard_image_target() {
            Some(ImagePasteTarget::Pane(current)) if current == terminal => {
                self.reset_displayed_scroll();
                self.pane.paste(path.as_bytes());
            }
            Some(ImagePasteTarget::Popup(current)) if current == terminal => {
                if let Some(popup) = self.popup.as_mut() {
                    popup.pane.paste(path.as_bytes());
                } else {
                    self.show_notice("Image pasted", path);
                }
            }
            _ => self.show_notice("Image pasted", path),
        }
    }
}

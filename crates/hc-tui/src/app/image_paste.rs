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

/// One image read in flight: the target it resolved, the gesture that
/// started it and the terminal input held behind it, in order. `held[0]`
/// is always that trigger — a miss replays it first so the program sees
/// the original input order.
pub(super) struct ImageRead {
    pub(super) seq: u64,
    pub(super) target: ImagePasteTarget,
    /// The hold bound: past it the read resolves as a miss and a late
    /// worker result lands dead.
    pub(super) deadline: Instant,
    pub(super) held: std::collections::VecDeque<UiEvent>,
}

/// A finished worker read. `seq` matches it to the in-flight read.
pub(super) struct ImageReadDone {
    pub(super) seq: u64,
    pub(super) read: crate::clipboard::ClipboardRead,
}

/// The clipboard-image reader seam — `Send + Sync` because the read
/// runs on a blocking worker thread.
pub(super) type ImageReader = Arc<dyn Fn() -> crate::clipboard::ClipboardRead + Send + Sync>;

/// The dropped-file reader seam — `Send + Sync` because the read runs
/// on a blocking worker thread.
pub(super) type DropReader =
    Arc<dyn Fn(&std::path::Path, &'static str) -> crate::clipboard::ClipboardRead + Send + Sync>;

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
    /// press starts an upload and is consumed; a re-dispatched repeat
    /// is swallowed only when the suppress set recorded its press.
    /// `None` means the key continues into normal routing — the program
    /// sees it unchanged, including an orphan repeat.
    pub(super) fn route_image_paste_key(&mut self, key: &input::TerminalKey) -> Option<()> {
        if self.image_read_replay || !self.image_paste_trigger_matches(key) {
            return None;
        }
        match key.input.action {
            hc_vt::Action::Repeat => self
                .image_paste_suppress
                .contains(&key.identity())
                .then_some(()),
            hc_vt::Action::Press if self.start_clipboard_image_paste(key) => {
                // Only a release-reporting host can ever send the
                // release — legacy hosts must not grow the set.
                if self.host_reports_key_releases {
                    self.image_paste_suppress.insert(key.identity());
                }
                Some(())
            }
            hc_vt::Action::Press => {
                // Matched but not consumed — drop any stale suppress
                // entry so a following repeat routes normally.
                self.image_paste_suppress.remove(&key.identity());
                None
            }
            _ => None,
        }
    }

    /// The key trigger: read the clipboard for the resolved target on a
    /// blocking worker. `true` consumed the press — it waits in `held`
    /// until the read says whether the program should see it.
    fn start_clipboard_image_paste(&mut self, key: &input::TerminalKey) -> bool {
        let Some(target) = self.clipboard_image_target() else {
            return false;
        };
        if !self.image_bridge_applies(target.terminal()) {
            return false;
        }
        let reader = Arc::clone(&self.image_paste_reader);
        self.start_image_read(target, UiEvent::Key(key.clone()), move || reader())
    }

    /// The paste-arm of `UiEvent::Paste`: an empty bracketed paste is
    /// the terminal's image-only Ctrl+Shift+V, and a single absolute
    /// image path is a file drop. `true` consumed the paste.
    pub(super) fn try_image_paste_bridge(&mut self, text: &[u8]) -> bool {
        if self.image_read_replay {
            return false;
        }
        let Some(target) = self.clipboard_image_target() else {
            return false;
        };
        if !self.image_bridge_applies(target.terminal()) {
            return false;
        }
        if text.is_empty() {
            let reader = Arc::clone(&self.image_paste_reader);
            self.start_image_read(target, UiEvent::Paste(text.to_vec()), move || reader())
        } else {
            let Ok(text) = std::str::from_utf8(text) else {
                return false;
            };
            let Some((path, extension)) = crate::clipdrop::image_path_from_paste(text) else {
                return false;
            };
            let reader = Arc::clone(&self.image_drop_reader);
            self.start_image_read(
                target,
                UiEvent::Paste(text.as_bytes().to_vec()),
                move || reader(&path, extension),
            )
        }
    }

    /// Reads run on a blocking worker, never on the loop: the gesture
    /// that started the read heads `held`, terminal input queues behind
    /// it in order, and the done channel returns the result by `seq`.
    /// One read at a time — a second gesture waits in `held`.
    fn start_image_read(
        &mut self,
        target: ImagePasteTarget,
        trigger: UiEvent,
        work: impl FnOnce() -> crate::clipboard::ClipboardRead + Send + 'static,
    ) -> bool {
        if let Some(read) = self.image_read.as_mut() {
            read.held.push_back(trigger);
            return true;
        }
        self.image_read_seq = self.image_read_seq.wrapping_add(1);
        let seq = self.image_read_seq;
        let done = self.image_read_done_tx.clone();
        self.image_read_task = Some(tokio::task::spawn_blocking(move || {
            let read = work();
            let _ = done.blocking_send(ImageReadDone { seq, read });
        }));
        self.image_read = Some(ImageRead {
            seq,
            target,
            deadline: Instant::now() + self.image_read_hold,
            held: std::collections::VecDeque::from([trigger]),
        });
        true
    }

    /// A worker's `ImageReadDone`: matching `seq` finishes the read —
    /// anything else is a late result after the bound already resolved
    /// the gesture and lands dead.
    pub(super) fn on_image_read_done(&mut self, done: ImageReadDone) {
        if self.image_read.as_ref().is_none_or(|r| r.seq != done.seq) {
            return;
        }
        let Some(read) = self.image_read.take() else {
            return;
        };
        self.image_read_task = None;
        self.finish_image_read(read, Some(done.read));
    }

    /// The hold bound: a never-returning worker resolves its gesture as
    /// a miss — the held trigger and keys replay in order and the
    /// eventual worker result lands dead on `seq`.
    pub(super) fn expire_image_read(&mut self, now: Instant) -> bool {
        if self
            .image_read
            .as_ref()
            .is_none_or(|read| now < read.deadline)
        {
            return false;
        }
        if let Some(task) = self.image_read_task.take() {
            task.abort();
        }
        let Some(read) = self.image_read.take() else {
            return false;
        };
        self.finish_image_read(read, None);
        true
    }

    /// Shared end of `on_image_read_done` and the expired bound:
    /// `Some(read)` dispatches like a live answer, `None` is the miss.
    /// A miss replays the trigger as ordinary input — `image_read_replay`
    /// keeps it from re-entering the bridge — then the held input in
    /// order. A read consumed its trigger; only the held input follows.
    fn finish_image_read(
        &mut self,
        read: ImageRead,
        result: Option<crate::clipboard::ClipboardRead>,
    ) {
        let mut held = read.held;
        let Some(trigger) = held.pop_front() else {
            return;
        };
        let consumed = match result {
            Some(result) => self.dispatch_image_read(read.target, result),
            None => {
                self.show_notice(
                    "No image to paste",
                    "The clipboard or dropped path has no readable image",
                );
                false
            }
        };
        if !consumed {
            // The press was not consumed after all: drop its suppress
            // entry so the coming release and repeats route normally,
            // then replay it past the bridge.
            if let UiEvent::Key(key) = &trigger
                && key.input.action == hc_vt::Action::Press
            {
                self.image_paste_suppress.remove(&key.identity());
            }
            self.image_read_replay = true;
            self.on_input(trigger, self.area.0);
            self.image_read_replay = false;
        }
        // Held input replays in order — unless a replayed gesture just
        // started the next read, in which case it queues behind that.
        for ev in held {
            if let Some(read) = self.image_read.as_mut() {
                read.held.push_back(ev);
            } else {
                self.on_input(ev, self.area.0);
            }
        }
    }

    /// Shared end of both triggers: upload an image, notice a too-large
    /// one, or explain the miss and let the gesture through when there
    /// is none — a paste that cannot work never fails silently.
    fn dispatch_image_read(
        &mut self,
        target: ImagePasteTarget,
        read: crate::clipboard::ClipboardRead,
    ) -> bool {
        match read {
            crate::clipboard::ClipboardRead::Image(image) => {
                self.submit_image_upload(target, image);
                true
            }
            crate::clipboard::ClipboardRead::TooLarge => {
                self.image_too_large_notice();
                true
            }
            crate::clipboard::ClipboardRead::Unavailable(reason) => {
                self.show_notice("Image paste unavailable", reason);
                false
            }
            crate::clipboard::ClipboardRead::None => {
                self.show_notice(
                    "No image to paste",
                    "The clipboard or dropped path has no readable image",
                );
                false
            }
        }
    }

    fn image_too_large_notice(&mut self) {
        self.show_notice(
            "Image too large",
            "The clipboard image exceeds the core's 8 MB upload limit",
        );
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
            self.image_too_large_notice();
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
                    self.show_notice("Image uploaded", path);
                }
            }
            _ => self.show_notice("Image uploaded", path),
        }
    }
}

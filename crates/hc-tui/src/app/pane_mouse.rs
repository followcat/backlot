// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
//! Content gestures stay with their original attach generation through release.
use hc_vt::{Mods, MouseAction, MousePosition};
use ratatui::layout::Position;

use super::input::{MouseButton, MouseEventKind, MouseInput};
use super::{App, Mode};
use crate::chrome::render::PaneHit;

pub(super) struct Gesture {
    hit: PaneHit,
    generation: u64,
    button: MouseButton,
    stripped: Mods,
    last: MouseInput,
    pixels: Option<crate::mouse_geometry::HostPixels>,
}

impl App {
    pub(super) fn sync_pixel_mouse(&mut self) {
        let child_capture = self.pane.vt().is_some_and(|vt| vt.mouse_tracking());
        let enabled = self.shell.mouse_capture || child_capture;
        let desired = self.host_sgr_pixel_mouse == Some(true)
            && self.host_facts.cell_px.is_some()
            && enabled
            && self
                .pane
                .vt()
                .is_some_and(|vt| vt.mode(1016).unwrap_or(false));
        if (desired != self.host_pixel_mouse_active || enabled != self.host_mouse_capture_active)
            && let Ok(sequence) = crate::terminal_modes::mouse_capture_sequence(enabled, desired)
            && (self.host_control)(&sequence).is_ok()
        {
            self.host_pixel_mouse_active = desired;
            self.host_mouse_capture_active = enabled;
        }
    }

    pub(super) fn on_host_mouse(&mut self, mut mouse: MouseInput) {
        self.host_mouse_pixels = None;
        if let Some((x, y)) = mouse.sgr_position {
            if self.host_pixel_mouse_active {
                let geometry = crate::mouse_geometry::HostGeometry::current().or_else(|| {
                    let (cw, ch) = self.host_facts.cell_px?;
                    crate::mouse_geometry::HostGeometry::new(
                        self.area.0,
                        self.area.1,
                        u32::from(self.area.0) * cw,
                        u32::from(self.area.1) * ch,
                    )
                });
                let Some(geometry) = geometry else {
                    return;
                };
                let Some((column, row)) = geometry.cell(x, y) else {
                    return;
                };
                mouse.col = column;
                mouse.row = row;
                self.host_mouse_pixels = Some(crate::mouse_geometry::HostPixels { x, y, geometry });
            } else if x > u32::from(u16::MAX) + 1 || y > u32::from(u16::MAX) + 1 {
                return;
            }
        }
        self.on_mouse(mouse);
        self.host_mouse_pixels = None;
    }

    /// Return true if pane input consumed the event before chrome routing.
    pub(super) fn route_pane_mouse(&mut self, mouse: MouseInput) -> bool {
        if let Some(mut gesture) = self.pane_mouse_gesture.take() {
            let matches = matches!(mouse.kind,
                MouseEventKind::Drag(button) | MouseEventKind::Up(button) if button == gesture.button);
            if matches {
                let hit = self
                    .hit_map
                    .panes
                    .iter()
                    .find(|hit| hit.pane_id == gesture.hit.pane_id)
                    .cloned()
                    .unwrap_or_else(|| gesture.hit.clone());
                if gesture.generation == self.pane.generation() {
                    let mut event = mouse;
                    event.mods = event.mods.difference(gesture.stripped);
                    self.send_pane_mouse(&hit, event);
                }
                gesture.last = mouse;
                gesture.pixels = self.host_mouse_pixels;
                if !matches!(mouse.kind, MouseEventKind::Up(_)) {
                    self.pane_mouse_gesture = Some(gesture);
                }
                return true;
            }
            self.pane_mouse_gesture = Some(gesture);
            if matches!(
                mouse.kind,
                MouseEventKind::Down(_) | MouseEventKind::Up(_) | MouseEventKind::Drag(_)
            ) {
                return true;
            }
        }
        if self.help {
            return false;
        }
        let Some(hit) = self
            .hit_map
            .panes
            .iter()
            .find(|hit| hit.inner_rect.contains(Position::new(mouse.col, mouse.row)))
            .cloned()
        else {
            return false;
        };
        if self.focused.as_deref() != Some(&hit.pane_id) {
            if matches!(
                mouse.kind,
                MouseEventKind::Down(MouseButton::Left)
                    | MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight
            ) {
                self.focus_pane(hit.pane_id.clone());
            } else {
                return true;
            }
        }
        let reporting = self
            .displayed_pane(&hit.pane_id)
            .and_then(|p| p.vt_ref())
            .is_some_and(|vt| vt.mouse_tracking());
        match mouse.kind {
            MouseEventKind::Down(button) if reporting => {
                let stripped = if button == MouseButton::Right {
                    // Source matches the configured modifier set exactly.
                    let Some(mods) = self
                        .shell
                        .right_click_passthrough_modifiers
                        .filter(|mods| *mods == mouse.mods)
                    else {
                        return true; // Context actions are M4.
                    };
                    mods
                } else {
                    Mods::NONE
                };
                let mut event = mouse;
                event.mods = event.mods.difference(stripped);
                self.send_pane_mouse(&hit, event);
                self.pane_mouse_gesture = Some(Gesture {
                    hit,
                    generation: self.pane.generation(),
                    button,
                    stripped,
                    last: mouse,
                    pixels: self.host_mouse_pixels,
                });
                true
            }
            MouseEventKind::Moved if reporting => {
                self.send_pane_mouse(&hit, mouse);
                true
            }
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                if reporting {
                    self.send_pane_mouse(&hit, mouse);
                } else if self
                    .displayed_pane(&hit.pane_id)
                    .and_then(|p| p.vt_ref())
                    .is_some_and(|vt| {
                        vt.alt_screen().unwrap_or(false) && vt.mode(1007).unwrap_or(false)
                    })
                {
                    let key = match mouse.kind {
                        MouseEventKind::ScrollUp => Some(hc_vt::Key::ArrowUp),
                        MouseEventKind::ScrollDown => Some(hc_vt::Key::ArrowDown),
                        _ => None,
                    };
                    if let Some(key) = key {
                        self.pane
                            .key(&super::input::TerminalKey::new(hc_vt::KeyInput {
                                key,
                                mods: Mods::NONE,
                                action: hc_vt::Action::Press,
                            }));
                    }
                } else {
                    let delta = match mouse.kind {
                        MouseEventKind::ScrollUp => -(self.shell.mouse_scroll_lines as isize),
                        MouseEventKind::ScrollDown => self.shell.mouse_scroll_lines as isize,
                        _ => 0,
                    };
                    if let Some(vt) = self.displayed_pane_mut(&hit.pane_id).and_then(|p| p.vt()) {
                        vt.scroll_delta(delta);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn send_pane_mouse(&mut self, hit: &PaneHit, mouse: MouseInput) {
        if self.mode != Mode::Normal {
            return;
        }
        // Source replays an unclaimed Ctrl-click after link activation without
        // its raw pixel metadata; the press therefore uses the cell center.
        let fallback_click = self.shell.mouse_capture
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && mouse.mods.contains(Mods::CTRL);
        let pixels = self.host_mouse_pixels.filter(|_| !fallback_click);
        let mut input = native_input(hit, mouse);
        if let Some(vt) = self.displayed_pane(&hit.pane_id).and_then(|p| p.vt_ref())
            && vt.mode(1016).unwrap_or(false)
        {
            let (cw, ch) = vt.cell_px();
            if let Some(crate::mouse_geometry::Position::Pixels { x, y }) = pixels.and_then(|p| {
                p.pane_position(
                    hit.inner_rect,
                    u32::from(hit.inner_rect.width) * cw,
                    u32::from(hit.inner_rect.height) * ch,
                )
            }) {
                input.position = MousePosition::Pixels { x, y };
            }
        }
        if self.pane.target() == Some(hit.pane_id.as_str()) {
            if mouse.kind != MouseEventKind::Moved {
                self.reset_displayed_scroll();
                self.pane.reset_scroll();
            }
            self.pane.mouse(input);
        }
    }

    /// Source releases the captured button before changing the target/focus.
    pub(super) fn release_pane_mouse(&mut self) {
        if let Some(gesture) = self.pane_mouse_gesture.take()
            && gesture.generation == self.pane.generation()
        {
            let mut mouse = gesture.last;
            mouse.kind = MouseEventKind::Up(gesture.button);
            mouse.mods = mouse.mods.difference(gesture.stripped);
            self.host_mouse_pixels = gesture.pixels;
            self.send_pane_mouse(&gesture.hit, mouse);
            self.host_mouse_pixels = None;
        }
    }
}

pub(super) fn native_input(hit: &PaneHit, mouse: MouseInput) -> hc_vt::MouseInput {
    use hc_vt::MouseButton as B;
    let button = |b| match b {
        MouseButton::Left => B::Left,
        MouseButton::Middle => B::Middle,
        MouseButton::Right => B::Right,
    };
    let (action, button) = match mouse.kind {
        MouseEventKind::Down(b) => (MouseAction::Press, Some(button(b))),
        MouseEventKind::Up(b) => (MouseAction::Release, Some(button(b))),
        MouseEventKind::Drag(b) => (MouseAction::Motion, Some(button(b))),
        MouseEventKind::Moved => (MouseAction::Motion, None),
        MouseEventKind::ScrollUp => (MouseAction::Press, Some(B::WheelUp)),
        MouseEventKind::ScrollDown => (MouseAction::Press, Some(B::WheelDown)),
        MouseEventKind::ScrollLeft => (MouseAction::Press, Some(B::WheelLeft)),
        MouseEventKind::ScrollRight => (MouseAction::Press, Some(B::WheelRight)),
    };
    hc_vt::MouseInput {
        action,
        button,
        mods: mouse.mods,
        position: MousePosition::Cell {
            column: mouse.col.saturating_sub(hit.inner_rect.x),
            row: mouse.row.saturating_sub(hit.inner_rect.y),
        },
    }
}

#[cfg(test)]
#[path = "pane_mouse_tests.rs"]
mod tests;

// Derived from herdr src/popup_size.rs (Apache-2.0).
//! Popup size model: a requested `width`/`height` is either a cell count or
//! a percent of the host surface; resolved geometry is clamped to the
//! surface and centered, with the same minimums as the reference.
use serde::Deserialize;

/// One dimension of a popup request: absolute cells, or a percent of the
/// available surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupSize {
    Cells(u16),
    Percent(u8),
}

impl PopupSize {
    /// Resolve against an available dimension (`(available * percent) / 100`).
    pub fn resolve(self, available: u16) -> u16 {
        match self {
            Self::Cells(cells) => cells,
            Self::Percent(percent) => available.saturating_mul(u16::from(percent)) / 100,
        }
    }

    /// Parse a config value: a string percent (`"80%"`) or an integer cell
    /// count — the shapes `[[keys.command]]` `width`/`height` may take.
    pub fn from_toml(value: &toml::Value) -> Result<Self, String> {
        match value {
            toml::Value::Integer(cells) => u16::try_from(*cells)
                .map(Self::Cells)
                .map_err(|_| format!("popup size {cells} out of range")),
            toml::Value::String(text) => {
                let percent = text
                    .strip_suffix('%')
                    .ok_or_else(|| format!("popup size {text:?} is not a percentage"))?
                    .parse::<u8>()
                    .map_err(|_| format!("popup size {text:?} is not a percentage"))?;
                if !(1..=100).contains(&percent) {
                    return Err(format!("popup percent {percent} out of range 1..=100"));
                }
                Ok(Self::Percent(percent))
            }
            _ => Err(format!(
                "popup size {value} must be cells or a percent string"
            )),
        }
    }
}

/// The centered popup rectangles: `outer` including its one-cell border,
/// `inner` the terminal surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupResolvedGeometry {
    pub outer: ratatui::layout::Rect,
    pub inner: ratatui::layout::Rect,
    /// Terminal width: outer minus 3 when wide, else minus 2 — see the
    /// reference's `terminal_width`.
    pub terminal_cols: u16,
    pub terminal_rows: u16,
}

pub const MIN_POPUP_WIDTH: u16 = 6;
pub const MIN_POPUP_HEIGHT: u16 = 4;

/// Resolve the popup geometry over `area`. With no request the popup is half
/// the surface in each dimension; a request is clamped to the surface and
/// the minimums. `None` when the surface cannot fit a minimum popup.
pub fn resolve_popup_geometry(
    width: Option<PopupSize>,
    height: Option<PopupSize>,
    area: ratatui::layout::Rect,
) -> Option<PopupResolvedGeometry> {
    if area.width < MIN_POPUP_WIDTH || area.height < MIN_POPUP_HEIGHT {
        return None;
    }
    let dimension = |requested: Option<PopupSize>, available: u16, minimum: u16| {
        requested
            .map(|size| size.resolve(available))
            .unwrap_or(available / 2)
            .clamp(minimum, available)
    };
    let outer_width = dimension(width, area.width, MIN_POPUP_WIDTH);
    let outer_height = dimension(height, area.height, MIN_POPUP_HEIGHT);
    let outer = ratatui::layout::Rect::new(
        area.x + (area.width - outer_width) / 2,
        area.y + (area.height - outer_height) / 2,
        outer_width,
        outer_height,
    );
    let inner = ratatui::layout::Rect::new(
        outer.x + 1,
        outer.y + 1,
        outer.width.saturating_sub(2),
        outer.height.saturating_sub(2),
    );
    let terminal_cols = if outer.width >= 8 {
        outer.width - 3
    } else {
        outer.width - 2
    };
    Some(PopupResolvedGeometry {
        outer,
        inner,
        terminal_cols,
        terminal_rows: inner.height,
    })
}

impl<'de> Deserialize<'de> for PopupSize {
    /// String values are percentages; numeric values are cell counts — the
    /// reference's own deserialization rule.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = PopupSize;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a percentage string or a cell count")
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<PopupSize, E> {
                u16::try_from(v).map(PopupSize::Cells).map_err(E::custom)
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<PopupSize, E> {
                u16::try_from(v).map(PopupSize::Cells).map_err(E::custom)
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<PopupSize, E> {
                let percent = v
                    .strip_suffix('%')
                    .ok_or_else(|| E::custom(format!("popup size {v:?} is not a percentage")))?
                    .parse::<u8>()
                    .map_err(E::custom)?;
                if !(1..=100).contains(&percent) {
                    return Err(E::custom(format!("popup percent {percent} out of range")));
                }
                Ok(PopupSize::Percent(percent))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(test)]
mod tests;

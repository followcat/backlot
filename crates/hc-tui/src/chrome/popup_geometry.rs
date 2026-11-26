//! Modal & menu geometry resolution: preset insets, gutter/padding shrink
//! ladder (`gutter -> padding -> content`), and hit-rect calculation.

use ratatui::layout::Rect;

/// Per-overlay-family visual treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupChrome {
    /// Dialogs & wizards: 1-cell outer gutter, rounded border, `2/1` padding,
    /// full backdrop dim (applied by caller), and 1-cell drop shadow.
    Standard,
    /// Border-joined panels (Reader, Task Sheet, Navigator): 1-cell outer
    /// gutter, rounded border, `0/0` border padding (body keeps its own
    /// horizontal margins & divider breathing room), dim + shadow.
    BorderJoined,
    /// PTY popups (Peek, Float): 1-cell outer gutter, rounded border,
    /// `0/0` inner padding (keeps PTY cell grid intact), dim + shadow.
    Terminal,
    /// Context menus: no full-screen dim, no outer gutter, rounded border,
    /// `pad_x = 1, pad_y = 0`, 1-cell drop shadow + CJK seam guard.
    Menu,
}

impl PopupChrome {
    pub(crate) const fn gutter(self) -> u16 {
        match self {
            Self::Menu => 0,
            Self::Standard | Self::BorderJoined | Self::Terminal => 1,
        }
    }

    pub(crate) const fn pad(self) -> (u16, u16) {
        match self {
            Self::Standard => (2, 1),
            Self::Menu => (1, 0),
            Self::BorderJoined | Self::Terminal => (0, 0),
        }
    }
}

/// Resolved rectangles for a popup: `outer` (gutter + border = hit rect,
/// excluding shadow), `border` (rounded frame edge), and `inner` (padded body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PopupGeometry {
    pub outer: Rect,
    pub border: Rect,
    pub inner: Rect,
}

/// Center desired `content_w × content_h` inside `host` following the shrink
/// ladder: outer gutter (`1 -> 0`) first, then padding (`2 -> 1 -> 0` / `1 -> 0`),
/// then content width/height.
pub(crate) fn resolve_centered(
    host: Rect,
    content_w: u16,
    content_h: u16,
    min_border_w: u16,
    min_border_h: u16,
    chrome: PopupChrome,
) -> Option<PopupGeometry> {
    let avail_w = host.width;
    let avail_h = host.height;
    if avail_w < min_border_w || avail_h < min_border_h {
        return None;
    }
    let (ideal_px, ideal_py) = chrome.pad();
    let ideal_g = chrome.gutter();
    let gutter = if ideal_g > 0
        && content_w + 2 * (ideal_g + 1 + ideal_px) <= avail_w
        && content_h + 2 * (ideal_g + 1 + ideal_py) <= avail_h
    {
        ideal_g
    } else {
        0
    };

    let mut pad_x = ideal_px;
    while pad_x > 0 && content_w + 2 * (gutter + 1 + pad_x) > avail_w {
        pad_x -= 1;
    }
    let mut pad_y = ideal_py;
    while pad_y > 0 && content_h + 2 * (gutter + 1 + pad_y) > avail_h {
        pad_y -= 1;
    }

    let inner_w = content_w.min(avail_w.saturating_sub(2 * (gutter + 1 + pad_x)));
    let inner_h = content_h.min(avail_h.saturating_sub(2 * (gutter + 1 + pad_y)));
    let border_w = inner_w + 2 + 2 * pad_x;
    let border_h = inner_h + 2 + 2 * pad_y;
    if border_w < min_border_w || border_h < min_border_h {
        return None;
    }
    let outer_w = border_w + 2 * gutter;
    let outer_h = border_h + 2 * gutter;
    let outer = Rect::new(
        host.x + (avail_w - outer_w) / 2,
        host.y + (avail_h - outer_h) / 2,
        outer_w,
        outer_h,
    );
    let border = Rect::new(outer.x + gutter, outer.y + gutter, border_w, border_h);
    let inner = Rect::new(border.x + 1 + pad_x, border.y + 1 + pad_y, inner_w, inner_h);
    Some(PopupGeometry {
        outer,
        border,
        inner,
    })
}

/// Center a desired border box `border_w × border_h` inside `host`, adding
/// outer gutter (`1 -> 0`) when `host` permits before shrinking border size.
pub(crate) fn resolve_from_border_size(
    host: Rect,
    border_w: u16,
    border_h: u16,
    min_border_w: u16,
    min_border_h: u16,
    chrome: PopupChrome,
) -> Option<PopupGeometry> {
    if host.width < min_border_w || host.height < min_border_h {
        return None;
    }
    let ideal_g = chrome.gutter();
    let gutter = if ideal_g > 0
        && border_w + 2 * ideal_g <= host.width
        && border_h + 2 * ideal_g <= host.height
    {
        ideal_g
    } else {
        0
    };
    let bw = border_w.min(host.width.saturating_sub(2 * gutter));
    let bh = border_h.min(host.height.saturating_sub(2 * gutter));
    if bw < min_border_w || bh < min_border_h {
        return None;
    }
    let (px, py) = shrink_pad_for_border(bw, bh, chrome.pad());
    let outer_w = bw + 2 * gutter;
    let outer_h = bh + 2 * gutter;
    let outer = Rect::new(
        host.x + (host.width - outer_w) / 2,
        host.y + (host.height - outer_h) / 2,
        outer_w,
        outer_h,
    );
    let border = Rect::new(outer.x + gutter, outer.y + gutter, bw, bh);
    let inner = Rect::new(
        border.x + 1 + px,
        border.y + 1 + py,
        bw.saturating_sub(2 + 2 * px),
        bh.saturating_sub(2 + 2 * py),
    );
    Some(PopupGeometry {
        outer,
        border,
        inner,
    })
}

/// Wrap an anchored `border` rect inside `host`, applying `chrome.pad()`
/// inside `border` (shrunk if needed) and `chrome.gutter()` when it fits.
pub(crate) fn wrap_border_in_host(host: Rect, border: Rect, chrome: PopupChrome) -> PopupGeometry {
    let ideal_g = chrome.gutter();
    let can_gutter = ideal_g > 0
        && border.x >= host.x + ideal_g
        && border.y >= host.y + ideal_g
        && border.right() + ideal_g <= host.right()
        && border.bottom() + ideal_g <= host.bottom();
    let g = if can_gutter { ideal_g } else { 0 };
    let outer = Rect::new(
        border.x - g,
        border.y - g,
        border.width + 2 * g,
        border.height + 2 * g,
    );
    let (px, py) = shrink_pad_for_border(border.width, border.height, chrome.pad());
    let inner = Rect::new(
        border.x + 1 + px,
        border.y + 1 + py,
        border.width.saturating_sub(2 + 2 * px),
        border.height.saturating_sub(2 + 2 * py),
    );
    PopupGeometry {
        outer,
        border,
        inner,
    }
}

#[inline]
fn shrink_pad_for_border(bw: u16, bh: u16, (mut px, mut py): (u16, u16)) -> (u16, u16) {
    while px > 0 && bw < 2 + 2 * px + 1 {
        px -= 1;
    }
    while py > 0 && bh < 2 + 2 * py + 1 {
        py -= 1;
    }
    (px, py)
}

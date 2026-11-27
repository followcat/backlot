//! Reader metadata model and Row 1 / Row 2 header layout for every
//! entry kind: Studio Exchange (`OUTCOME`/`REPORT`/`TELL`), Hollywood
//! exchange (`REPORT`/`TELL`), Actionable Cards (`CARD`), Task Sheet
//! artifacts (`CHECK`/`DELIVERY`/`REQ`), Actor Records (`RECORDS`),
//! and Opening Design (`DESIGN`).

use backlot_model::summary::{HollywoodSummary, RoleSummary, StudioSummary, Work};
use ratatui::style::{Color, Style};

use super::reader_design::nav_token;
use crate::chrome::palette::Palette;
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

/// What kind of entry the Reader currently displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ReaderKind {
    #[default]
    None,
    Outcome,
    Report,
    Tell,
    Card,
    Check,
    Delivery,
    Req,
    Records,
    Design,
}

/// Originating list for `←`/`→` and `[`/`]` sibling entry stepping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum ReaderOrigin {
    #[default]
    None,
    StudioExchange(StudioKey, u64),
    HollywoodExchange(u64),
    Card(String),
    SheetDeliveries(StudioKey, Option<String>),
    Records(StudioKey),
    Design(StudioKey),
}

/// Structured header and action context attached to a [`CardView`] or `SheetView`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReaderMeta {
    pub kind: ReaderKind,
    pub studio: Option<String>,
    pub machine: Option<String>,
    pub seq: Option<u64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub dropped_reason: Option<String>,
    pub task: Option<String>,
    pub file: Option<String>,
    pub ts_ms: Option<u64>,
    /// Ledger role key (`producer`, `actor-alice`, `hollywood`).
    pub role: Option<String>,
    /// Visual state label (`ready`, `waiting`, `working`, `blocked`, `lost`, `closed`, ...).
    pub role_state: Option<String>,
    /// Live role terminal address if currently running (used by Slice 4 focus/peek).
    pub role_terminal: Option<String>,
    /// Whether the author role currently holds a user-owned prompt.
    pub attention_user: bool,
    pub runner: Option<String>,
    pub model: Option<String>,
    pub resumable: bool,
    /// 1-based position and total count across navigable siblings (`◀ i/N ▶`).
    pub nav_index: Option<usize>,
    pub nav_total: Option<usize>,
    /// In-flight sibling load marker shown in Row 1 as `■ loading <label>…`.
    pub loading_label: Option<String>,
    /// Target origin for an in-flight stepped load so stale out-of-order replies drop cleanly.
    pub loading_origin: Option<ReaderOrigin>,
    /// Card proposal summary for Row 2 (`hollywood proposes: <action>`).
    pub card_action: Option<String>,
    /// Card status token (`open`, etc.).
    pub card_status: Option<String>,
    /// Sheet delivery/check status token (`✓ units · exit 0`, `FAIL (exit 1)`).
    pub check_status: Option<String>,
    /// Records studio counts: `(tasks_done, tasks_total, roles_len)`.
    pub studio_counts: Option<(u32, u32, usize)>,
    /// Whether the target studio is already finished.
    pub studio_finished: bool,
    /// Active design version (`design/v<N>.md`).
    pub design_version: Option<u32>,
    /// Active design discussion round (`R<r>`).
    pub design_round: Option<u32>,
    /// Active design discussion phase (`proposal`, `review`, `summary`, `waiting on you`, ...).
    pub design_phase: Option<backlot_model::design::DesignPhase>,
    /// Optional design sign-off mode (`Approved` / `Skipped`) once finalized.
    pub design_sign_mode: Option<backlot_model::design::SignMode>,
    /// Current section label shown in the design reader top bar.
    pub design_section: Option<String>,
    /// Explicit active section index when jumping with `[` / `]` in Design Reader.
    pub design_section_idx: Option<usize>,
    pub origin: ReaderOrigin,
}

/// `[o]` behavior for a Reader entry when an author role is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReaderOAction {
    Focus,
    Reopen,
}

/// Fixed studio seats that remain reopenable even when closed without a saved session.
pub(crate) fn is_fixed_seat(role: &str) -> bool {
    matches!(role, "producer" | "director" | "writer")
        || (!role.starts_with("actor") && role != "hollywood")
}

impl ReaderMeta {
    /// Available `[o]` action: focus live terminal, reopen closed resumable/fixed seat, or none.
    pub(crate) fn o_action(&self) -> Option<ReaderOAction> {
        if self.role_state.as_deref() == Some("closed") {
            let can_reopen = matches!(self.origin, ReaderOrigin::StudioExchange(..))
                && self
                    .role
                    .as_deref()
                    .is_some_and(|r| self.resumable || is_fixed_seat(r));
            can_reopen.then_some(ReaderOAction::Reopen)
        } else if self.role_terminal.is_some() {
            Some(ReaderOAction::Focus)
        } else {
            None
        }
    }

    /// Whether `[p] peek` is available (requires a non-closed role with a live terminal).
    pub(crate) fn can_peek(&self) -> bool {
        self.role_state.as_deref() != Some("closed") && self.role_terminal.is_some()
    }

    /// Whether `[t] tell` is available on this entry.
    pub(crate) fn can_tell(&self) -> bool {
        matches!(
            self.kind,
            ReaderKind::Outcome
                | ReaderKind::Report
                | ReaderKind::Tell
                | ReaderKind::Check
                | ReaderKind::Delivery
                | ReaderKind::Req
        ) || (self.kind == ReaderKind::Records && !self.studio_finished)
            || (self.kind == ReaderKind::Design && !self.design_finalized())
    }

    /// Whether this design document is finalized/signed (`Signed` or `-final.md`).
    pub(crate) fn design_finalized(&self) -> bool {
        super::reader_design::design_finalized(self)
    }

    /// Whether this design document awaits user sign-off (`WaitingUser` or `SuggestSkip`).
    pub(crate) fn design_awaiting_user(&self) -> bool {
        super::reader_design::design_awaiting_user(self)
    }

    /// Originating studio for `[v] sheet` (None for Hollywood entries or when already inside Sheet).
    pub(crate) fn sheet_studio_key(&self) -> Option<StudioKey> {
        match &self.origin {
            ReaderOrigin::StudioExchange(k, _)
            | ReaderOrigin::Records(k)
            | ReaderOrigin::Design(k) => Some(k.clone()),
            ReaderOrigin::Card(_) => self.studio.as_ref().map(|id| StudioKey {
                machine: self.machine.clone(),
                id: id.clone(),
            }),
            ReaderOrigin::SheetDeliveries(_, _)
            | ReaderOrigin::HollywoodExchange(_)
            | ReaderOrigin::None => None,
        }
    }
}

/// Format an age token from `ts_ms` relative to `now_ms`:
/// `<60s` -> `now`, `<60m` -> `<m>m`, `<24h` -> `<h>h`, else `<d>d`.
pub(crate) fn age_label(ts_ms: Option<u64>, now_ms: u64) -> Option<String> {
    let ts = ts_ms?;
    let secs = now_ms.saturating_sub(ts) / 1000;
    Some(if secs < 60 {
        "now".to_string()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    })
}

/// Square-only semantic state mark (`■` / `□`) and palette color.
pub(crate) fn square_state_mark(state: &str, palette: &Palette) -> (&'static str, Color) {
    match state {
        "live" | "ready" | "waiting" => ("■", palette.green),
        "working" => ("■", palette.yellow),
        "blocked" | "lost" | "failed" | "needs you" => ("■", palette.red),
        _ => ("□", palette.overlay0),
    }
}

/// Visual state label derived from a `RoleSummary` via shared `Person::of` rules.
pub(crate) fn role_state_word(role: &RoleSummary) -> &'static str {
    match Person::of(role) {
        Person::Working => "working",
        Person::Ready => "ready",
        Person::Idle | Person::Slot => "waiting",
        Person::Lost => "lost",
        Person::UserPrompt
        | Person::DirPrompt
        | Person::NeedsYou
        | Person::Asks
        | Person::Stuck
        | Person::Failed => "blocked",
        Person::Resumable | Person::Closed | Person::Standby => "closed",
    }
}

/// Visual state label derived from `HollywoodSummary` status fields.
pub(crate) fn hollywood_state_word(h: &HollywoodSummary) -> &'static str {
    match h.state.as_str() {
        "live" => {
            if h.attention_user || h.work == Some(Work::Blocked) {
                "blocked"
            } else if h.work == Some(Work::Working) {
                "working"
            } else {
                "ready"
            }
        }
        "lost" => "lost",
        _ => "closed",
    }
}

/// Visual state word derived from optional `StudioSummary` role lookup.
pub(crate) fn find_studio_role<'a>(
    studio: &'a StudioSummary,
    name_or_role: &str,
) -> Option<&'a RoleSummary> {
    studio
        .roles
        .iter()
        .find(|r| r.role == name_or_role || r.name == name_or_role)
}

pub(super) fn join_non_empty(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn display_actor(raw: &str) -> &str {
    match raw {
        "user" => "you",
        other => other,
    }
}

/// Resolved header strings for Row 1 (top border) and Row 2 (context line).
pub(crate) struct HeaderLayout {
    pub left: String,
    pub left_wide: Option<String>,
    pub right: String,
    pub nav_token: Option<String>,
    pub row2_left: String,
    pub row2_state: Option<(String, &'static str, Color)>,
    pub row2_right: String,
    pub has_row2: bool,
}

impl HeaderLayout {
    pub(crate) fn build(
        title: &str,
        card: Option<&str>,
        m: &ReaderMeta,
        now_ms: u64,
        palette: &Palette,
        plain: bool,
    ) -> Self {
        if m.kind == ReaderKind::None {
            return Self {
                left: title.to_string(),
                left_wide: None,
                right: String::new(),
                nav_token: None,
                row2_left: String::new(),
                row2_state: None,
                row2_right: String::new(),
                has_row2: false,
            };
        }

        let (left, left_wide) = if let Some(loading) = m.loading_label.as_deref() {
            (format!("■ loading {loading}…"), None)
        } else if !plain && m.kind == ReaderKind::Design {
            let (compact, wide) = super::reader_design::row1_left_babylon(m);
            (compact, Some(wide))
        } else {
            (format_row1_left(title, card, m), None)
        };

        let nav_token = if m.kind == ReaderKind::Design {
            nav_token(m)
        } else {
            match (m.nav_index, m.nav_total) {
                (Some(i), Some(n)) if n > 0 && m.kind != ReaderKind::Records => {
                    Some(format!("◀ {i}/{n} ▶"))
                }
                _ => None,
            }
        };
        let age = if m.kind == ReaderKind::Records {
            None
        } else {
            age_label(m.ts_ms, now_ms)
        };
        let machine_right = (m.kind != ReaderKind::Card)
            .then(|| m.machine.clone())
            .flatten();
        let right = join_non_empty([machine_right, nav_token.clone(), age]);

        let (row2_left, row2_state, row2_right) = build_row2(m, palette);

        Self {
            left,
            left_wide,
            right,
            nav_token,
            row2_left,
            row2_state,
            row2_right,
            has_row2: true,
        }
    }

    pub(crate) fn row2_state_style(&self) -> Option<(&str, Style, &str)> {
        self.row2_state
            .as_ref()
            .map(|(word, glyph, color)| (*glyph, Style::default().fg(*color), word.as_str()))
    }
}

fn format_row1_left(title: &str, card: Option<&str>, m: &ReaderMeta) -> String {
    match m.kind {
        ReaderKind::None => title.to_string(),
        ReaderKind::Outcome | ReaderKind::Report | ReaderKind::Tell => {
            let tag = match m.kind {
                ReaderKind::Outcome => "OUTCOME",
                ReaderKind::Report => "REPORT",
                ReaderKind::Tell => "TELL",
                _ => unreachable!(),
            };
            let head = match m.seq {
                Some(seq) => format!("■ {tag} #{seq}"),
                None => format!("■ {tag}"),
            };
            join_non_empty([Some(head), m.studio.clone()])
        }
        ReaderKind::Card => {
            let card_id = card
                .map(|id| format!("■ CARD {id}"))
                .unwrap_or_else(|| "■ CARD".to_string());
            join_non_empty([Some(card_id), m.studio.clone(), m.card_status.clone()])
        }
        ReaderKind::Check | ReaderKind::Delivery | ReaderKind::Req => {
            let tag = match m.kind {
                ReaderKind::Check => "■ CHECK",
                ReaderKind::Delivery => "■ DELIVERY",
                ReaderKind::Req => "■ REQ",
                _ => unreachable!(),
            };
            join_non_empty([Some(tag.to_string()), m.studio.clone()])
        }
        ReaderKind::Records => join_non_empty([Some("■ RECORDS".to_string()), m.studio.clone()]),
        ReaderKind::Design => super::reader_design::row1_left(m),
    }
}

fn build_row2(
    m: &ReaderMeta,
    palette: &Palette,
) -> (String, Option<(String, &'static str, Color)>, String) {
    match m.kind {
        ReaderKind::None => (String::new(), None, String::new()),
        ReaderKind::Outcome | ReaderKind::Report | ReaderKind::Tell => {
            let route = match (m.from.as_deref(), m.to.as_deref()) {
                (Some(f), Some(t)) => Some(format!("{} → {}", display_actor(f), display_actor(t))),
                (Some(f), None) => Some(display_actor(f).to_string()),
                (None, Some(t)) => Some(display_actor(t).to_string()),
                (None, None) => None,
            };
            let dropped = m.dropped_reason.as_deref().map(|r| format!("dropped: {r}"));
            let left = join_non_empty([route, dropped, m.runner.clone(), m.model.clone()]);
            let state = m.role_state.as_deref().map(|st| {
                let (glyph, color) = square_state_mark(st, palette);
                (st.to_string(), glyph, color)
            });
            let right = join_non_empty([m.task.clone(), m.file.clone()]);
            (left, state, right)
        }
        ReaderKind::Card => {
            let left = m
                .card_action
                .as_deref()
                .map(|act| format!("hollywood proposes: {act}"))
                .unwrap_or_default();
            let right = m.machine.clone().unwrap_or_default();
            (left, None, right)
        }
        ReaderKind::Check | ReaderKind::Delivery | ReaderKind::Req => {
            let task = m.task.as_deref().map(|t| format!("task {t}"));
            let left = join_non_empty([task, m.check_status.clone()]);
            let right = m.file.clone().unwrap_or_default();
            (left, None, right)
        }
        ReaderKind::Records => {
            let left = m
                .studio_counts
                .map(|(done, total, roles)| format!("{done}/{total} tasks · {roles} roles"))
                .unwrap_or_default();
            (left, None, String::new())
        }
        ReaderKind::Design => super::reader_design::row2(m, palette),
    }
}

//! The `›` line's behaviour: `i`/`/` land the selection on a
//! block's input row (the explicit `editing` latch does the rest),
//! `⏎` sends a `tell` to the producer, and a leading `/` runs the
//! deterministic verbs through the same `StudioMenu`/`StudioAct`
//! arm the context menu uses — menu and command line can never
//! disagree about what a verb does (S3 §1's single arm).

use backlot_model::event::Priority;

use crate::panel::Target;
use crate::studio::StudioKey;
use crate::studio_cmds;
use crate::studio_menu::{StudioAct, StudioMenu};

use super::App;

/// The `/`-verbs' one-line summary — the notice a bad verb or
/// `/help` prints, in the menu's own order.
const SLASH_HELP: &str = "/tell <role> [msg] · /stop /start /finish /revise /records \
     · /focus /sheet · /peek /close /reopen /clear <role> \
     · /pause /resume /abandon /takeover <task> · /priority <task> low|normal|high · /menu";

/// The deterministic verbs the `›` line runs — `Tab` completes into
/// this list and the mistype guard matches against it.
const SLASH_VERBS: &[&str] = &[
    "tell", "stop", "start", "finish", "pause", "resume", "abandon", "takeover", "revise",
    "records", "focus", "sheet", "peek", "close", "reopen", "clear", "priority", "menu", "help",
];

/// Whether `line` is one stray ASCII letter glued to a `/verb` —
/// `i/tell producer hi`, `j/pause t1` — the fat-fingered compose a
/// resting `i`/`j`/`k` left in the buffer. A guarded line is never
/// sent to the producer and never dropped (R3-B2).
fn mistyped_verb(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b'/' {
        return false;
    }
    let verb = line[2..]
        .split(|ch: char| ch.is_whitespace())
        .next()
        .unwrap_or("");
    SLASH_VERBS.contains(&verb)
}

impl App {
    /// `i`/`/` on a focused dock — select the `›` row of the
    /// selection's studio (the current studio when the cursor sits
    /// on a machine/stage row, else the first block's) and arm its
    /// `Compose`. `i` keeps a parked draft — or seeds `/tell <role> `
    /// on a person row with none; `/` seeds the deterministic prefix.
    pub(super) fn panel_input_focus(&mut self, slash: bool) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let selected = targets.get(index).cloned();
        let person_role = match &selected {
            Some(Target::Person(_, role)) => Some(role.clone()),
            _ => None,
        };
        let key = selected
            .as_ref()
            .and_then(|t| t.studio().cloned())
            .or_else(|| self.studio.current.clone())
            .or_else(|| {
                crate::panel::data::studios(&machines)
                    .first()
                    .map(|s| s.key())
            });
        let Some(key) = key else {
            return; // no studio anywhere — nothing to tell
        };
        // A finished studio has no `›` row — `i`/`/` stay inert there.
        let Some(row) = self
            .panel_targets()
            .iter()
            .position(|t| matches!(t, Target::Input(k) if *k == key))
        else {
            return;
        };
        let draft = self.panel.draft_mut(&key);
        let seed = match (slash, &person_role) {
            // `/` starts the verb cleanly; an existing `/`-draft
            // keeps what it had.
            (true, _) if draft.as_str().is_empty() => Some("/".to_owned()),
            (true, _) => None,
            (false, Some(role)) if draft.as_str().is_empty() => Some(format!("/tell {role} ")),
            (false, _) => None,
        };
        if let Some(seed) = seed {
            draft.clear();
            let _ = draft.insert(&seed);
        }
        self.panel.selected = Some(Target::Input(key));
        self.panel.selected_row = Some(row);
        self.panel.editing = true;
    }

    /// `⏎` on the `›` line: a plain line is a `tell` to the
    /// producer, a `/`-line is the verb parser, an empty line just
    /// rests. A send clears the studio's draft — a rejected
    /// mistyped `/verb` keeps it — and the row falls back to
    /// `Resting` either way (R3-2's dock rule), so `j`/`k`/`m`/`Esc`
    /// act on the dock at once.
    /// Sync one studio's Compose history from `StudioSummary.exchange`.
    pub(super) fn panel_sync_history(&mut self, key: &StudioKey) {
        let machines = self.panel_summaries();
        if let Some(studio) = crate::panel::data::studio(&machines, key) {
            self.panel.history_mut(key).sync(
                studio
                    .studio
                    .exchange
                    .iter()
                    .map(|e| (e.seq, e.who.as_str(), e.text.as_str())),
            );
        }
    }

    pub(super) fn panel_input_submit(&mut self) {
        let Some(Target::Input(key)) = self.panel.selected.clone() else {
            return;
        };
        let line = self
            .panel
            .draft(&key)
            .map(|draft| draft.as_str().trim().to_string())
            .unwrap_or_default();
        if line.is_empty() {
            self.panel.editing = false;
            self.panel.reset_history_nav();
            return;
        }
        if mistyped_verb(&line) {
            self.reject_input("looks like a mistyped /command — remove the leading letter or edit");
            return;
        }
        self.panel_sync_history(&key);
        if line != "/" {
            self.panel.history_mut(&key).record_send(&line);
        } else {
            self.panel.reset_history_nav();
        }
        match line.strip_prefix('/') {
            Some(rest) => self.panel_slash(&key, rest),
            None => self.run_action(studio_cmds::tell(&key, "producer", &line)),
        }
        self.panel.clear_draft(&key);
        self.panel.editing = false;
    }

    /// `Tab` on a `/verb` prefix: the shared stem extends, a unique
    /// verb completes with its trailing space — anything else is a
    /// no-op (spec.md's `/verb` completion, R3-B2).
    pub(super) fn panel_input_complete(&mut self) {
        let Some(Target::Input(key)) = self.panel.selected.clone() else {
            return;
        };
        let draft = self.panel.draft_mut(&key);
        let Some(prefix) = draft.as_str().strip_prefix('/') else {
            return;
        };
        // A verb prefix only — once args follow, Tab has nothing to
        // finish.
        if prefix.contains(' ') {
            return;
        }
        let matches: Vec<&'static str> = SLASH_VERBS
            .iter()
            .copied()
            .filter(|verb| verb.starts_with(prefix))
            .collect();
        let completion = match matches.as_slice() {
            [one] => format!("/{one} "),
            [] => return,
            many => {
                let common = many[1..].iter().fold(many[0], |stem: &str, verb| {
                    let shared = stem
                        .chars()
                        .zip(verb.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    &stem[..shared]
                });
                if common.len() == prefix.len() {
                    return;
                }
                format!("/{common}")
            }
        };
        draft.clear();
        let _ = draft.insert(&completion);
    }

    /// One `/verb` on the input row's studio. The verbs name the
    /// same acts the `m` menu lists and run through the same
    /// `run_studio_act` arm — a reason-needing verb (`/pause t1`)
    /// opens its reason dialog, a destructive one (`/stop`,
    /// `/close`) its confirm, the rest run straight.
    fn panel_slash(&mut self, key: &StudioKey, rest: &str) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            self.show_notice("Unknown studio", "the studio row is gone");
            return;
        };
        let offline = !self.machine_reachable(studio.machine);
        let stage_down = !self.machine_stage_running(studio.machine);
        let roles = &studio.studio.roles;
        let tasks = &studio.studio.tasks;
        let studio_menu = || StudioMenu::studio(key, studio.studio, stage_down, offline);
        let (verb, args) = rest
            .split_once(' ')
            .map_or((rest, ""), |(v, a)| (v, a.trim()));
        let role_menu = |role: &str| {
            roles
                .iter()
                .find(|r| r.role == role)
                .map(|r| (StudioMenu::role(key, r, offline), r.clone()))
        };
        match verb {
            // Bare `/` opens the same `m` menu the verb list mirrors —
            // the deterministic surface's front door.
            "" | "menu" => {
                let point = self
                    .panel_hits
                    .rows
                    .iter()
                    .find(|(_, t)| *t == Target::Input(key.clone()))
                    .map(|(rect, _)| (rect.x + 1, rect.y + 1))
                    .unwrap_or((self.panel_hits.area.x + 1, self.panel_hits.area.y + 2));
                self.open_studio_menu(studio_menu().inside_dock(), point);
            }
            "help" | "?" => self.show_notice("/ verbs", SLASH_HELP),
            "tell" => {
                let (role, msg) = args
                    .split_once(' ')
                    .map_or((args, ""), |(r, m)| (r, m.trim()));
                if role.is_empty() {
                    self.run_studio_act(&studio_menu(), StudioAct::Tell);
                } else if msg.is_empty() {
                    match role_menu(role) {
                        Some((menu, _)) => self.run_studio_act(&menu, StudioAct::Tell),
                        None => self
                            .show_notice("No such role", &format!("{role} is not in {}", key.id)),
                    }
                } else {
                    self.run_action(studio_cmds::tell(key, role, msg));
                }
            }
            "focus" => self.run_studio_act(&studio_menu(), StudioAct::FocusTerminal),
            "sheet" => self.run_studio_act(&studio_menu(), StudioAct::Sheet),
            "records" => self.run_studio_act(&studio_menu(), StudioAct::Records),
            "revise" => self.run_studio_act(&studio_menu(), StudioAct::Revise),
            "finish" => self.run_studio_act(&studio_menu(), StudioAct::Finish),
            "start" => self.run_studio_act(&studio_menu(), StudioAct::StageStart),
            "stop" => self.run_studio_act(&studio_menu(), StudioAct::StageStop),
            "peek" | "close" | "reopen" | "clear" => {
                // No argument means the first role with an open
                // attention — the `clear` family's natural read.
                let role = if args.is_empty() {
                    roles
                        .iter()
                        .find(|r| r.attention_seq.is_some())
                        .map(|r| r.role.as_str())
                } else {
                    args.split_whitespace().next()
                };
                let Some((menu, summary)) = role.and_then(&role_menu) else {
                    self.show_notice(
                        "No such role",
                        &format!(
                            "{} is not a role in {}",
                            if args.is_empty() { "nobody" } else { args },
                            key.id
                        ),
                    );
                    return;
                };
                let act = match verb {
                    "peek" => StudioAct::Peek,
                    "close" => StudioAct::Close,
                    "reopen" => StudioAct::Reopen,
                    _ => StudioAct::ClearAttention,
                };
                if act == StudioAct::ClearAttention && summary.attention_seq.is_none() {
                    self.show_notice(
                        "Nothing to clear",
                        &format!("{} has no open attention", summary.name),
                    );
                    return;
                }
                self.run_studio_act(&menu, act);
            }
            "pause" | "resume" | "abandon" | "takeover" => {
                let verb: &'static str = match verb {
                    "pause" => "pause",
                    "resume" => "resume",
                    "abandon" => "abandon",
                    _ => "takeover",
                };
                let task = args.split_whitespace().next().unwrap_or("");
                match tasks.iter().find(|t| t.id == task) {
                    Some(task) => {
                        let menu = StudioMenu::task(key, task, offline);
                        self.run_studio_act(&menu, StudioAct::TaskVerb(verb));
                    }
                    None => self.show_notice(
                        "No such task",
                        &format!("{task} is not a task in {}", key.id),
                    ),
                }
            }
            "priority" => {
                let mut parts = args.split_whitespace();
                let (task, level) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let level = match level {
                    "high" => Some(Priority::High),
                    "normal" => Some(Priority::Normal),
                    "low" => Some(Priority::Low),
                    _ => None,
                };
                match (tasks.iter().find(|t| t.id == task), level) {
                    (Some(task), Some(level)) => {
                        let menu = StudioMenu::task(key, task, offline);
                        self.run_studio_act(&menu, StudioAct::Priority(level));
                    }
                    _ => self.show_notice("Bad priority", "/priority <task> low|normal|high"),
                }
            }
            _ => self.show_notice(&format!("No /{verb}"), SLASH_HELP),
        }
    }
}

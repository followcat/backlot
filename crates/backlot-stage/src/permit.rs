//! Permission prompts (D9): a live role sampled `blocked` on two
//! samples two seconds apart raises one `permission prompt`
//! attention per blocked episode, with the visible screen saved to
//! `attention/<seq>.txt`. An actor's episode owes the director a
//! notice pointing at that file; any other role's says it needs the
//! user. A recorded `role.approval` sends the runner's approval key
//! only while the prompt still shows — never without a recorded
//! decision — then clears the attention; a prompt already gone
//! clears with no key sent.

use backlot_model::event::{ApprovalDecision, PERMISSION_PROMPT};
use backlot_model::state::{ApprovalRecord, Attention, StudioState, actor_task};
use backlot_model::summary::Work;

use crate::runners::work;
use crate::watch::trust_dialog;

/// Two samples this far apart make a blocked episode — the same
/// window the send gate's screen stability uses.
pub const EPISODE_MIN_MS: u64 = 2_000;

/// Whether this sample sits inside a permission episode: a blocked
/// screen that is not the folder-trust dialog — the watch's trust
/// pass is already answering that one, so it neither starts nor
/// holds an episode. `work` still reads `blocked` for the panel.
pub fn in_episode(work: Work, screen: &str) -> bool {
    work == Work::Blocked && !trust_dialog(screen)
}

/// The episode bookkeeping for one fresh sample: inside an episode
/// the start holds (or begins now) and the raise is due at
/// [`EPISODE_MIN_MS`]; outside it the episode is over — a prompt
/// after the dialog starts its own count.
pub fn episode_due(in_episode: bool, since: Option<u64>, now_ms: u64) -> (Option<u64>, bool) {
    if !in_episode {
        return (None, false);
    }
    let since = since.unwrap_or(now_ms);
    (Some(since), now_ms.saturating_sub(since) >= EPISODE_MIN_MS)
}

/// The key a recorded decision sends while the prompt still shows:
/// `enter` takes the highlighted allow option, `esc` dismisses. That
/// is measured on `fake` (the byte stream is asserted in tests) and
/// observed on `devin` 3000.12.32; `claude` and `codex` are mapped
/// from their pickers and still to be measured in S5b. Any other
/// runner is unmeasured and answers nothing — the open attention
/// stays for the user. docs/studio/runners.md says which are
/// measured.
pub fn approval_keys(runner: &str, decision: ApprovalDecision) -> Option<&'static str> {
    match runner {
        "fake" | "claude" | "codex" | "devin" => Some(match decision {
            ApprovalDecision::Allow => "enter",
            ApprovalDecision::Deny => "esc",
        }),
        _ => None,
    }
}

/// The region of a blocked screen that names *this* prompt — what a
/// recorded decision may answer, compared against the block of the
/// screen the attention saved. `None` when the runner's prompt
/// markers are not on the screen: a runner the stage can answer
/// then sends nothing. Blocks are whitespace-normalised and drop
/// lines that move while the prompt waits — Claude's spinner
/// carries a running timer.
pub fn prompt_block(runner: &str, screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen.lines().collect();
    let start = match runner {
        // From the last full-width `─` rule above `Do you want to
        // proceed?` to the end: the tool header, the description,
        // the command between `╌` rules and the picker.
        "claude" => {
            let ask = lines
                .iter()
                .rposition(|l| l.contains("Do you want to proceed?"))?;
            lines[..ask].iter().rposition(|l| is_full_width_rule(l))?
        }
        // From the last `fake-work: blocked` line to the end — a
        // `fake-prompt:` line under it is what tells two prompts
        // apart.
        "fake" => lines
            .iter()
            .rposition(|l| l.trim_end() == "fake-work: blocked")?,
        // The question line to the end — the command the picker asks
        // about sits under it. Unmeasured, see docs/studio/runners.md.
        "codex" => lines
            .iter()
            .rposition(|l| l.contains("Would you like to"))?,
        // From the last `●` line above `Approval required` to the
        // end — the `●` block holds the command the picker asks
        // about, and the judge's sentence, the `❭` cursor and the
        // scope word (`Once`, `Session`, …) stay in the block: a
        // cursor or scope that moved under a recorded decision
        // refuses the send instead of answering a shifted picker.
        "devin" => {
            let ask = lines
                .iter()
                .rposition(|l| l.contains("Approval required"))?;
            lines[..ask]
                .iter()
                .rposition(|l| l.trim_start().starts_with('●'))?
        }
        _ => return None,
    };
    let block = lines[start..]
        .iter()
        .filter(|l| !(runner == "claude" && work::claude_spinner_line(l)))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    Some(block.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A separator drawn the full width of the screen: only `─` after
/// trimming. The `╌` rules inside the prompt block — the ones around
/// the command — are a different character and stay inside it.
fn is_full_width_rule(l: &str) -> bool {
    let t = l.trim();
    t.chars().count() >= 3 && t.chars().all(|c| c == '─')
}

/// The role's open `permission prompt` attention, when it has one.
pub fn open_episode<'s>(state: &'s StudioState, role: &str) -> Option<&'s Attention> {
    state
        .open_attention()
        .find(|a| a.role == role && a.reason == PERMISSION_PROMPT)
}

/// The latest decision a role recorded at or after the open
/// attention — the one answering *this* prompt. An approval from an
/// earlier episode is spent: the stage must never send a key for a
/// decision that pre-dates the prompt on screen.
pub fn pending_approval<'s>(
    state: &'s StudioState,
    role: &str,
    attention_ts_ms: u64,
) -> Option<&'s ApprovalRecord> {
    state
        .approvals
        .iter()
        .rev()
        .find(|a| a.role == role && a.ts_ms >= attention_ts_ms)
}

/// Whether the role's current blocked episode already produced an
/// attention: one with a timestamp at or after the episode's start
/// covers it — open or cleared. The stage learns the episode start
/// from its own samples, so a restart starts a new episode.
pub fn episode_raised(state: &StudioState, role: &str, blocked_since_ms: u64) -> bool {
    state
        .attention
        .iter()
        .any(|a| a.role == role && a.reason == PERMISSION_PROMPT && a.ts_ms >= blocked_since_ms)
}

/// Whether the role key is an actor — only actors' episodes notify
/// the director; anyone else's needs the user.
pub fn is_actor(role: &str) -> bool {
    actor_task(role).is_some()
}

/// Whether this screen's prompt is the user's to answer (S9b): a
/// role that is not an actor is always the user's, and an actor's
/// prompt is the user's when its own block reaches for `BACKLOT_*`
/// or an environment-clearing flag. The flag reads a hook signal's
/// command when the prompt came from one (S9f), else the prompt
/// block when the runner maps one — an earlier prompt's line still
/// scrolling above it must not flag the new prompt — and the whole
/// screen when it maps none.
pub fn needs_user(role: &str, runner: &str, screen: &str, command: Option<&str>) -> bool {
    !is_actor(role)
        || touches_env(
            &command.map(String::from).unwrap_or_else(|| {
                prompt_block(runner, screen).unwrap_or_else(|| screen.to_string())
            }),
        )
}

/// Whether a saved prompt screen reaches for the role-identity
/// variables or the environment-clearing env flags — the S9b "the
/// prompt is the user's" flag. The screen is flattened first
/// (whitespace and box-drawing characters removed) so a wrapped
/// `BACKLOT_RO\nLE` inside a prompt box still matches; a command
/// that asks for `BACKLOT_` anything, runs `env -i` or passes
/// `--ignore-environment` is trying to change who it runs as, and
/// only the user may say yes.
pub fn touches_env(screen: &str) -> bool {
    let flat = flattened(screen);
    flat.contains("BACKLOT_") || flat.contains("env-i") || flat.contains("--ignore-environment")
}

/// The screen with every whitespace and box-drawing character
/// removed: prompt boxes wrap long commands across lines and draw a
/// border, so matching looks at the flattened text — a `│` between
/// wrapped halves of a word cannot hide it.
fn flattened(screen: &str) -> String {
    screen
        .chars()
        .filter(|c| !c.is_whitespace() && !is_box_drawing(*c))
        .collect()
}

/// The box-drawing and block range a prompt box's border is made
/// of: `─│┌┐└┘├┤╭╮╰╯█` and friends.
fn is_box_drawing(c: char) -> bool {
    matches!(c, '\u{2500}'..='\u{259F}')
}

/// The director's notice for an actor's episode, pointing at the
/// saved screen — `None` for any other role, whose attention says
/// it needs the user, and for a flagged screen (`needs_user`),
/// which is the user's to answer.
pub fn director_notice(attn: &Attention, detail_file: &str) -> Option<String> {
    if attn.needs_user || !is_actor(&attn.role) {
        return None;
    }
    Some(format!(
        "# permission prompt on {}\n\n`{}` is waiting on a permission prompt; its screen was saved to `{detail_file}`.\n\nApprove only when the prompt stays inside that actor's own work area — a prompt touching real sessions, production, accounts or global settings, or destructive outside the work area, goes to the user.\n\n`backlot approve --person {} --decision allow` (or `deny`)\n",
        attn.role, attn.role, attn.role,
    ))
}

//! The D9 pure pieces: which key a decision sends per runner, the
//! open-episode and pending-approval lookups, the per-episode
//! dedup, and who a permission attention notifies.

use std::path::Path;

use backlot_model::event::{ApprovalDecision, PERMISSION_PROMPT};
use backlot_model::state::{ApprovalRecord, Attention, StudioState};
use backlot_model::summary::Work;

use crate::notices;
use crate::permit::{
    CursorAsk, approval_keys, cursor_ask, cursor_needs_user, director_notice, episode_due,
    episode_raised, in_episode, is_actor, needs_user, open_episode, pending_approval, prompt_block,
    touches_env,
};

fn attn(seq: u64, role: &str, open: bool, ts_ms: u64) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: PERMISSION_PROMPT.into(),
        detail: format!("screen saved to attention/{seq}.txt"),
        open,
        ts_ms,
        needs_user: false,
        cleared_reason: None,
    }
}

/// `allow` presses enter and `deny` presses esc on the runners whose
/// pickers share that convention — `devin` measured on 3000.12.32;
/// an unmeasured runner answers nothing — the attention stays open
/// for the user.
#[test]
fn keys_per_runner() {
    for runner in ["fake", "claude", "codex", "devin"] {
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Allow),
            Some("enter"),
            "{runner}"
        );
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Deny),
            Some("esc"),
            "{runner}"
        );
    }
    for runner in ["opencode", "unknown"] {
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Allow),
            None,
            "{runner}"
        );
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Deny),
            None,
            "{runner}"
        );
    }
}

/// cursor's allow is `y` — the `→ Run (once) (y)` picker's key — and
/// a deny sends nothing: S11a showed the runner retries a rejected
/// command, so no key could close the prompt anyway. Neither
/// decision may ever map to `tab` or `shift+tab` — those are the
/// picker's allowlist entries and would write the config.
#[test]
fn keys_per_runner_cursor() {
    assert_eq!(approval_keys("cursor", ApprovalDecision::Allow), Some("y"));
    assert_eq!(approval_keys("cursor", ApprovalDecision::Deny), None);
    for decision in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        assert!(
            !matches!(approval_keys("cursor", decision), Some("tab" | "shift+tab")),
            "{decision:?} must never reach the allowlist entries"
        );
    }
}

/// `open_episode` finds only an open `permission prompt` for the
/// role — a cleared attention or another reason does not count.
#[test]
fn open_episode_is_open_and_reasoned() {
    let mut state = StudioState::default();
    assert!(open_episode(&state, "actor-t1-c1").is_none());
    state.attention.push(attn(4, "actor-t1-c1", false, 10));
    assert!(open_episode(&state, "actor-t1-c1").is_none());
    state.attention.push(attn(6, "actor-t1-c1", true, 20));
    assert_eq!(open_episode(&state, "actor-t1-c1").unwrap().seq, 6);
    assert!(open_episode(&state, "director").is_none());
    let mut other = attn(8, "writer", true, 30);
    other.reason = "rate limit".into();
    state.attention.push(other);
    assert!(open_episode(&state, "writer").is_none());
}

/// The latest recorded decision at or after the open attention is
/// the one the stage acts on; an approval from an earlier episode
/// is spent and never answers a new prompt.
#[test]
fn pending_approval_is_latest_and_current() {
    let mut state = StudioState::default();
    assert!(pending_approval(&state, "actor-t1-c1", 0).is_none());
    state.approvals.push(ApprovalRecord {
        seq: 3,
        role: "actor-t1-c1".into(),
        by: "director".into(),
        decision: ApprovalDecision::Allow,
        ts_ms: 1,
    });
    state.approvals.push(ApprovalRecord {
        seq: 5,
        role: "actor-t1-c1".into(),
        by: "director".into(),
        decision: ApprovalDecision::Deny,
        ts_ms: 2,
    });
    assert_eq!(
        pending_approval(&state, "actor-t1-c1", 1).unwrap().decision,
        ApprovalDecision::Deny
    );
    // A prompt raised at t=10 has no answer yet: both approvals are
    // from the earlier episode.
    assert!(pending_approval(&state, "actor-t1-c1", 10).is_none());
    assert!(pending_approval(&state, "writer", 0).is_none());
}

/// The folder-trust dialog is not a permission episode — the trust
/// pass is already answering it. Held past two seconds it raises
/// nothing, for an actor and for the director, and it ends an
/// episode in progress: a prompt right after it needs its own
/// full two seconds.
#[test]
fn trust_dialog_is_no_permission_episode() {
    let trust = "Quick safety check\n\n ❯ Yes, I trust this folder\n   No, exit\n";
    let prompt = "Do you want to proceed?\n❯ 1. Yes\n";
    for role in ["actor-t1-c1", "director"] {
        assert!(!in_episode(Work::Blocked, trust), "{role}");
        let mut since = None;
        for now in [0, 1_000, 2_500] {
            let (s, due) = episode_due(in_episode(Work::Blocked, trust), since, now);
            assert!(!due, "{role} at {now}");
            since = s;
        }
        assert_eq!(since, None, "{role}: the dialog opened an episode");
    }
    // A prompt right after the dialog starts the count fresh.
    let (since, due) = episode_due(in_episode(Work::Blocked, prompt), None, 3_000);
    assert_eq!(since, Some(3_000));
    assert!(!due);
    let (since, due) = episode_due(in_episode(Work::Blocked, prompt), since, 4_999);
    assert!(!due);
    let (_, due) = episode_due(in_episode(Work::Blocked, prompt), since, 5_000);
    assert!(due);
}

/// An episode is raised once: an attention at or after the episode
/// start covers it, open or cleared; one from before it does not.
#[test]
fn episode_dedups_on_attention_time() {
    let mut state = StudioState::default();
    state.attention.push(attn(4, "actor-t1-c1", false, 100));
    // Episode starting after the old attention: not covered.
    assert!(!episode_raised(&state, "actor-t1-c1", 200));
    state.attention.push(attn(6, "actor-t1-c1", true, 250));
    assert!(episode_raised(&state, "actor-t1-c1", 200));
    // A different role's attention never covers this episode.
    state.attention.push(attn(8, "writer", true, 300));
    assert!(episode_raised(&state, "actor-t1-c1", 200));
    assert!(!episode_raised(&state, "writer", 400));
}

/// Only `actor-<task>-c<n>` keys are actors — a director or writer
/// at a prompt is the user's concern.
#[test]
fn actor_keys() {
    assert!(is_actor("actor-t1-c1"));
    assert!(is_actor("actor-big-job-c12"));
    assert!(!is_actor("director"));
    assert!(!is_actor("writer"));
    assert!(!is_actor("actor-")); // empty task
}

/// The director is told about an actor's episode with the saved
/// screen's path and the approve command; a non-actor gets none —
/// and a flagged `needs_user` attention gets none either, actor or
/// not (S9b).
#[test]
fn director_notice_is_actor_only() {
    let a = attn(9, "actor-t1-c1", true, 10);
    let body = director_notice(&a, "/state/studios/acme-shop/attention/9.txt").unwrap();
    assert!(body.contains("actor-t1-c1"));
    assert!(body.contains("/state/studios/acme-shop/attention/9.txt"));
    assert!(body.contains("backlot approve --person actor-t1-c1 --decision allow"));
    let w = attn(11, "writer", true, 10);
    assert!(director_notice(&w, "/state/studios/acme-shop/attention/11.txt").is_none());
    let mut flagged = attn(13, "actor-t1-c1", true, 10);
    flagged.needs_user = true;
    assert!(director_notice(&flagged, "/state/studios/acme-shop/attention/13.txt").is_none());
}

/// The S9b screen scan: any reach for a `BACKLOT_*` variable, `env
/// -i` or `--ignore-environment` flags the prompt as the user's —
/// plain and inside a prompt box alike; an ordinary `cargo build`
/// prompt does not.
#[test]
fn touches_env_flags_the_identity_reaches() {
    for screen in [
        "BACKLOT_ROLE=director backlot review --task t1",
        "env -u BACKLOT_ROLE backlot review --task t1",
        "unset BACKLOT_ROLE; backlot review --task t1",
        "export BACKLOT_STUDIO=other backlot status",
        "env -i backlot status",
        "env -i BACKLOT_ROLE=user backlot status",
        "sudo --ignore-environment backlot review --task t1",
    ] {
        assert!(touches_env(screen), "{screen:?}");
    }
    for screen in [
        "cargo build --workspace",
        "rm -rf work/t1",
        "backlot status",
        "environmental review of the changes",
    ] {
        assert!(!touches_env(screen), "{screen:?}");
    }
}

/// A prompt box wraps a long command mid-word and draws a border —
/// the flattened text still carries the variable, so `BACKLOT_RO`
/// `LE` across lines is the same reach.
#[test]
fn touches_env_sees_through_a_wrapped_box() {
    let wrapped = "╭────────────────────────────────────────────╮\n\
                   │ Bash command                               │\n\
                   │   BACKLOT_RO                               │\n\
                   │   LE=director backlot review --task t1     │\n\
                   │                                            │\n\
                   │ Do you want to proceed?                    │\n\
                   │ ❯ 1. Yes                                   │\n\
                   ╰────────────────────────────────────────────╯";
    assert!(touches_env(wrapped));
    let wrapped_flag = "┌─────────────────────────┐\n\
                        │   env -i \u{2028}backlot      │\n\
                        │   status                  │\n\
                        └─────────────────────────┘";
    assert!(touches_env(wrapped_flag));
}

/// The `needs_user` flag reads the prompt's own block, not the
/// whole screen: a `BACKLOT_*` line left scrolling above a Claude
/// prompt's rule is an earlier prompt's tail, not this prompt's —
/// while a reach inside the block still flags, and a runner with no
/// block falls back to the whole screen.
#[test]
fn needs_user_reads_the_prompt_block() {
    let clean = claude_screen("cargo test --lib", "✻ Orbiting… (4s · ↓ 9 tokens)");
    assert!(!needs_user("actor-t1-c1", "claude", &clean, None));
    // `BACKLOT_*` above the rule is not in the block.
    let scrolled = format!("BACKLOT_ROLE=director backlot review --task t1\n{clean}");
    assert!(!needs_user("actor-t1-c1", "claude", &scrolled, None));
    // Inside the block it still flags.
    let reached = claude_screen(
        "BACKLOT_ROLE=director backlot review --task t1",
        "✻ Orbiting… (4s · ↓ 9 tokens)",
    );
    assert!(needs_user("actor-t1-c1", "claude", &reached, None));
    // No block mapped: the whole screen decides.
    assert!(needs_user("actor-t1-c1", "opencode", &scrolled, None));
    assert!(!needs_user(
        "actor-t1-c1",
        "opencode",
        "plain output\n",
        None
    ));
    // A non-actor is the user's regardless.
    assert!(needs_user("writer", "claude", &clean, None));
}

/// S9f: a `PermissionRequest` signal's own command is what the flag
/// reads — the screen never shows the request's command, so a reach
/// inside it flags while a plain command on a plain screen does not.
#[test]
fn needs_user_reads_the_request_command() {
    let plain = "plain output\n";
    assert!(!needs_user(
        "actor-t1-c1",
        "claude",
        plain,
        Some("cargo test --lib")
    ));
    assert!(needs_user(
        "actor-t1-c1",
        "claude",
        plain,
        Some("env -i BACKLOT_ROLE=director backlot review --task t1")
    ));
    // No command — a file-path request reads the path: it never
    // reaches for the identity variables, so it does not flag.
    assert!(!needs_user(
        "actor-t1-c1",
        "claude",
        plain,
        Some("/home/user/acme-shop/src/lib.rs")
    ));
}

/// A flagged attention owes the director no notice — the user owns
/// the prompt.
#[test]
fn a_flagged_attention_owes_no_director_notice() {
    let mut state = StudioState::default();
    let mut flagged = attn(7, "actor-t1-c1", true, 10);
    flagged.needs_user = true;
    state.attention.push(flagged);
    let dir = Path::new("/state/studios/acme-shop");
    assert!(notices::owed(&state, dir).iter().all(|n| n.source_seq != 7));
}

/// Owed notices: an actor's permission attention owes the director
/// one notice, deduped on `source_seq`; a non-actor's owes none.
#[test]
fn attention_notice_owed_once() {
    let mut state = StudioState::default();
    state.attention.push(attn(7, "actor-t1-c1", true, 10));
    state.attention.push(attn(8, "writer", true, 10));
    let dir = Path::new("/state/studios/acme-shop");
    let got = notices::owed(&state, dir);
    let actor_notices: Vec<_> = got.iter().filter(|n| n.source_seq == 7).collect();
    assert_eq!(actor_notices.len(), 1);
    assert_eq!(actor_notices[0].to, "director");
    assert!(actor_notices[0].body.contains("attention/7.txt"));
    assert!(got.iter().all(|n| n.source_seq != 8));
    // Once a message carries that source_seq, nothing is owed.
    state.messages.push(backlot_model::state::Message {
        seq: 9,
        from: "stage".into(),
        to: "director".into(),
        file: Some("messages/9-attention.md".into()),
        purpose: None,
        source_seq: Some(7),
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    });
    assert!(notices::owed(&state, dir).iter().all(|n| n.source_seq != 7));
}

/// A neutral Claude-shaped permission screen: the last full-width
/// `─` rule above `Do you want to proceed?` opens the region that
/// names the prompt — the ` Bash command` header, the description,
/// the command between `╌` rules and the picker — while the
/// spinner's running timer moves under it.
fn claude_screen(command: &str, spinner: &str) -> String {
    format!(
        "compacting old output\n\
         ─────────────────────────────────────────\n\
         \x20Bash command\n\
         \x20 The command a task asked to run\n\
         ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\n\
         \x20 {command}\n\
         ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\n\
         \x20Do you want to proceed?\n\
         \x20❯ 1. Yes\n\
         \x20  2. Yes, and don't ask again for this command\n\
         \x20  3. No\n\
         {spinner}\n"
    )
}

/// The claude block: the spinner's running timer and token count do
/// not move it — the same prompt between two samples reads as the
/// same block — and a different command is a different block.
#[test]
fn prompt_block_on_claude() {
    let a = claude_screen(
        "rm -rf ./build",
        "✻ Orbiting… (12s · ↓ 95 tokens · esc to interrupt)",
    );
    let later = claude_screen(
        "rm -rf ./build",
        "✻ Orbiting… (47s · ↓ 2 tokens · esc to interrupt)",
    );
    let block_a = prompt_block("claude", &a).expect("a block");
    assert_eq!(block_a, prompt_block("claude", &later).unwrap());
    assert!(block_a.contains("rm -rf ./build"), "{block_a}");
    assert!(block_a.contains("Do you want to proceed?"), "{block_a}");
    assert!(!block_a.contains("Orbiting"), "{block_a}");
    let other = claude_screen(
        "cargo test --lib",
        "✻ Orbiting… (12s · ↓ 95 tokens · esc to interrupt)",
    );
    assert_ne!(
        prompt_block("claude", &a),
        prompt_block("claude", &other),
        "a different command must be a different block"
    );
    // Whitespace moves do not move the block.
    let spaced = a.replacen("rm -rf ./build", "  rm   -rf   ./build  ", 1);
    assert_eq!(prompt_block("claude", &a), prompt_block("claude", &spaced));
    // Without the full-width rule there is no comparable block.
    assert_eq!(
        prompt_block("claude", "Do you want to proceed?\n❯ 1. Yes\n"),
        None
    );
    // Without the question there is none either.
    assert_eq!(
        prompt_block("claude", &a.replace("Do you want to proceed?", "")),
        None
    );
}

/// The fake block: from the last `fake-work: blocked` line to the
/// end — a `fake-prompt:` line under it is what makes two prompts
/// differ; anything above the last blocked line is out of it.
#[test]
fn prompt_block_on_fake() {
    let waiting = "fake runner: actor-t1-c1 of acme-shop\nfake-work: waiting\n";
    assert_eq!(prompt_block("fake", waiting), None);
    let a = "fake runner: actor-t1-c1 of acme-shop\n\
             fake-work: blocked\n\
             fake-prompt: rm -rf build\n";
    let b = "fake runner: actor-t1-c1 of acme-shop\n\
             fake-work: blocked\n\
             fake-prompt: rm -rf build\n\
             fake-approved: enter\n\
             fake-work: waiting\n\
             fake-work: blocked\n\
             fake-prompt: cargo test --lib\n";
    let block_a = prompt_block("fake", a).unwrap();
    assert_eq!(block_a, "fake-work: blocked fake-prompt: rm -rf build");
    assert_ne!(
        block_a,
        prompt_block("fake", b).unwrap(),
        "the second prompt's block is its own"
    );
    assert_eq!(
        prompt_block("fake", b).unwrap(),
        "fake-work: blocked fake-prompt: cargo test --lib"
    );
}

/// A neutral cursor-shaped prompt screen (the S11a capture): the
/// `$ <cmd>` line under a full-width `─` rule opens the block, the
/// picker ends it.
fn cursor_screen(command: &str) -> String {
    format!(
        "  $ backlot studios 1.2s\n\
         \x20   backlot stub studios\n\
         ─────────────────────────────────────────\n\
         \x20$  {command} in .\n\n\
         \x20Run this command?\n\
         \x20Not in allowlist: wc\n\
         \x20 → Run (once) (y)\n\
         \x20   Add Shell(wc) to allowlist? (tab)\n\
         \x20   Run Everything (shift+tab)\n\
         \x20   Skip & tell the agent what to do instead (esc or n)\n"
    )
}

/// The cursor block runs from the `─` rule above `Run this
/// command?` to the end — a different command is a different block,
/// the transcript above the rule is out of it, and without the rule
/// or the question there is nothing comparable.
#[test]
fn prompt_block_on_cursor() {
    let a = cursor_screen("wc -l README.md");
    let b = cursor_screen("cargo test --lib");
    let block = prompt_block("cursor", &a).expect("cursor block");
    assert!(block.contains("wc -l README.md"), "{block}");
    assert!(block.contains("Run this command?"), "{block}");
    assert!(!block.contains("backlot stub"), "{block}");
    assert_ne!(
        block,
        prompt_block("cursor", &b).unwrap(),
        "a different command must be a different block"
    );
    assert_eq!(prompt_block("cursor", "plain output\n"), None);
    assert_eq!(
        prompt_block(
            "cursor",
            " Run this command?\n Not in allowlist: wc\n  → Run (once) (y)\n"
        ),
        None,
        "no full-width rule — nothing comparable"
    );
}

/// A neutral cursor-shaped file-approval picker (the S12e capture,
/// scrubbed): the `Approval N of M` header under a full-width `─`
/// rule, the diff preview behind its inner rule, the question, the
/// ` in <path>` line and the option list every picker shares.
fn cursor_file_picker(question: &str, path: &str) -> String {
    format!(
        "some output\n\
         ─────────────────────────────────────────\n\
         \x20  Approval 1 of 3\n\
         \x20  ...{path} -1\n\
         \x20  ─────────────────────────\n\
         \x20  - an old line\n\
         \x20  + a new line\n\
         \x20{question}\n\
         \x20 in {path}\n\
         \x20 → Proceed (y)\n\
         \x20   Reject & propose changes (esc or n or p)\n\
         \x20   Add Write({path}) to allowlist? (tab)\n\
         \x20   Run Everything (shift+tab)\n\
         \x20 ←/→ to switch approval\n"
    )
}

/// The file picker's block opens at the inner `─` rule above the
/// `→` cursor — the diff preview is part of what was approved — and
/// a different question or target is a different block.
#[test]
fn prompt_block_on_cursor_file_picker() {
    let a = cursor_file_picker("Write to this file?", "/home/user/work/t1/plan.md");
    let block = prompt_block("cursor", &a).expect("a file picker has a block");
    assert!(block.contains("Write to this file?"), "{block}");
    assert!(block.contains("in /home/user/work/t1/plan.md"), "{block}");
    assert!(block.contains("a new line"), "{block}");
    assert!(!block.contains("some output"), "{block}");
    let other_path = cursor_file_picker("Write to this file?", "/home/user/work/t1/other.md");
    assert_ne!(
        prompt_block("cursor", &a),
        prompt_block("cursor", &other_path)
    );
    let other_question = cursor_file_picker("Delete this file?", "/home/user/work/t1/plan.md");
    assert_ne!(
        prompt_block("cursor", &a),
        prompt_block("cursor", &other_question)
    );
    // A screen with no `─` rule above the option cursor maps no
    // block — the user's raise already follows.
    assert_eq!(
        prompt_block(
            "cursor",
            "Write to this file?\n in /home/user/x\n → Proceed (y)\n Run Everything (shift+tab)\n"
        ),
        None
    );
}

/// `cursor_ask` reads the picker's own question: the shell prompt,
/// each file question with its `Add <Verb>(<path>)` or ` in <path>`
/// target, an unreadable target, and a picker naming nothing
/// measured. Off a picker screen there is nothing to read.
#[test]
fn cursor_ask_reads_the_pickers_question() {
    let shell = cursor_screen("wc -l README.md");
    assert_eq!(cursor_ask(&shell), Some(CursorAsk::Shell));
    for question in [
        "Write to this file?",
        "Write this file?",
        "Read this file?",
        "Delete this file?",
        "Proceed with this edit?",
    ] {
        let screen = cursor_file_picker(question, "/home/user/work/t1/plan.md");
        assert_eq!(
            cursor_ask(&screen),
            Some(CursorAsk::File(Some("/home/user/work/t1/plan.md"))),
            "{question}"
        );
    }
    // The ` in <path>` line alone still names the target when the
    // allowlist option is not offered.
    let no_add = cursor_file_picker("Read this file?", "/home/user/work/t1/src/lib.rs").replace(
        "   Add Write(/home/user/work/t1/src/lib.rs) to allowlist? (tab)\n",
        "",
    );
    assert_eq!(
        cursor_ask(&no_add),
        Some(CursorAsk::File(Some("/home/user/work/t1/src/lib.rs")))
    );
    // A truncated `...` path counts unread.
    let truncated = cursor_file_picker("Write to this file?", "/home/user/work/t1/plan.md")
        .replace(
            "Add Write(/home/user/work/t1/plan.md) to allowlist?",
            "Add Write(.../t1/plan.md) to allowlist?",
        )
        .replace(" in /home/user/work/t1/plan.md", " in .../t1/plan.md");
    assert_eq!(cursor_ask(&truncated), Some(CursorAsk::File(None)));
    // A picker whose question is not measured — an mcp prompt, say.
    let unknown = "Run this mcp?\n → Proceed (y)\n   Run Everything (shift+tab)\n";
    assert_eq!(cursor_ask(unknown), Some(CursorAsk::Unknown));
    // Off a picker there is nothing to read.
    assert_eq!(cursor_ask("plain output\n"), None);
}

/// A `Write to this file?` on an actor is the director's only while
/// the named path resolves strictly inside the role's own work
/// area — `.git`, `.commandcode`, `.cursor` and `cursor` state, an
/// outside or unresolvable path, a truncated one, no path at all
/// and an unmeasured picker are each the user's outright.
#[test]
fn cursor_file_picker_needs_user_only_outside_the_area() {
    let dir = std::env::temp_dir().join(format!("bl-permit-area-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let area = dir.join("work/t1");
    std::fs::create_dir_all(&area).unwrap();
    let inside = area.join("plan.md").display().to_string();
    assert!(
        !cursor_needs_user(
            "cursor",
            &cursor_file_picker("Write to this file?", &inside),
            &area
        ),
        "an in-worktree write is the director's to answer"
    );
    for path in [
        dir.join("other.md"),                      // beside the worktree
        area.join("../t2/x.md"),                   // `..` escaping it
        area.join(".git/HEAD"),                    // VCS state
        area.join(".commandcode/settings.json"),   // runner state
        area.join(".cursor/config.json"),          // cursor state
        area.join("cursor/chats/x/meta.json"),     // cursor state
        area.clone(),                              // the area itself is not inside itself
        Path::new("~/work/t1/x.md").to_path_buf(), // `~` — unexpandable
        Path::new("relative/x.md").to_path_buf(),  // relative — where from is unread
    ] {
        let screen = cursor_file_picker("Write to this file?", &path.display().to_string());
        assert!(
            cursor_needs_user("cursor", &screen, &area),
            "{} must be the user's",
            path.display()
        );
    }
    // Unreadable or unmeasured asks are the user's too.
    let truncated = cursor_file_picker("Write to this file?", &inside)
        .replace(&format!("Add Write({inside})"), "Add Write(.../plan.md)")
        .replace(&format!(" in {inside}"), " in .../plan.md");
    assert!(cursor_needs_user("cursor", &truncated, &area));
    let no_path = cursor_file_picker("Delete this file?", &inside)
        .replace(&format!(" in {inside}\n"), "")
        .replace(&format!("   Add Write({inside}) to allowlist? (tab)\n"), "");
    assert!(cursor_needs_user("cursor", &no_path, &area));
    let unknown = "Run this mcp?\n → Proceed (y)\n   Run Everything (shift+tab)\n";
    assert!(cursor_needs_user("cursor", unknown, &area));
    // Other runners and the shell picker are read elsewhere.
    assert!(!cursor_needs_user("claude", unknown, &area));
    assert!(!cursor_needs_user(
        "cursor",
        &cursor_screen("wc -l x"),
        &area
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A symlink inside the area pointing out still leaves the target
/// outside: the check resolves what the filesystem can.
#[test]
#[cfg(unix)]
fn cursor_file_picker_target_through_a_symlink_is_the_users() {
    let dir = std::env::temp_dir().join(format!("bl-permit-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let area = dir.join("work/t1");
    let outside = dir.join("outside");
    std::fs::create_dir_all(&area).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, area.join("link")).unwrap();
    let via_link = area.join("link/x.md").display().to_string();
    assert!(
        cursor_needs_user(
            "cursor",
            &cursor_file_picker("Write to this file?", &via_link),
            &area
        ),
        "a symlink pointing out is outside"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A cursor workspace-trust dialog is the user's even on an actor —
/// the stage launches with `--trust`, so the dialog showing at all
/// is a user decision, never something the stage answers.
#[test]
fn cursor_trust_dialog_is_the_users() {
    let trust = "  │  ⚠ Workspace Trust Required\n\
                 \x20 │  Do you trust the contents of this directory?\n\
                 \x20 │    /home/user/acme-shop\n\
                 \x20 │    [a] Trust this workspace\n\
                 \x20 │    [q] Quit\n";
    assert!(needs_user("actor-t1-c1", "cursor", trust, None));
    assert!(needs_user("director", "cursor", trust, None));
}

/// cmd's allow is `enter` on the highlighted `❯ 1. Yes` (run-once);
/// `esc` denies — the turn ENDS, no retry. Option `2` — "don't ask
/// again" — is never a decision the stage sends.
#[test]
fn keys_per_runner_cmd() {
    assert_eq!(approval_keys("cmd", ApprovalDecision::Allow), Some("enter"));
    assert_eq!(approval_keys("cmd", ApprovalDecision::Deny), Some("esc"));
    for decision in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        assert!(!matches!(
            approval_keys("cmd", decision),
            Some("2" | "down")
        ));
    }
}

/// A neutral cmd-shaped prompt screen (the S12a capture): the
/// full-width `─` rule opens the block, the `❯ 1.` picker ends it.
fn cmd_screen(command: &str) -> String {
    format!(
        "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n\
         ─────────────────────────────────────────\n\
         Execute Shell Command\n\
         Command Code needs to execute this shell command:\n\
         \x20 {command}\n\
         ❯ 1. Yes\n\
         \x20 2. Yes, don't ask again for `cmd` commands in this project\n\
         \x20 3. No, tell Command Code what to do differently\n"
    )
}

/// The cmd block runs from the last `─` rule above the `❯ 1.`
/// picker to the end — a different command is a different block,
/// and without the rule or the picker there is nothing comparable.
#[test]
fn prompt_block_on_cmd() {
    let a = cmd_screen("touch allow-ok.txt");
    let b = cmd_screen("rm scratch.txt");
    let block = prompt_block("cmd", &a).expect("cmd block");
    assert!(block.contains("touch allow-ok.txt"), "{block}");
    assert!(block.contains("needs to execute"), "{block}");
    assert!(!block.contains("Conjuring"), "{block}");
    assert_ne!(
        block,
        prompt_block("cmd", &b).unwrap(),
        "a different command must be a different block"
    );
    assert_eq!(prompt_block("cmd", "plain output\n"), None);
    assert_eq!(
        prompt_block("cmd", "Execute Shell Command\nneeds to execute\n❯ 1. Yes\n"),
        None,
        "no full-width rule — nothing comparable"
    );
}

/// A cmd folder-trust dialog is the user's even on an actor — the
/// stage launches with `--trust`, so the dialog showing at all is
/// a user decision, never something the stage answers.
#[test]
fn cmd_trust_dialog_is_the_users() {
    let trust = "  Do you trust the files in this folder?\n\
                 \x20 /home/user/acme-shop\n\
                 \x20 ❯ 1. Yes, proceed\n\
                 \x20   2. No, exit\n";
    assert!(needs_user("actor-t1-c1", "cmd", trust, None));
    assert!(needs_user("director", "cmd", trust, None));
    // And its rounded box is not a prompt block.
    assert_eq!(prompt_block("cmd", trust), None);
}

/// Codex blocks come from their own prompt marker — unmeasured, but
/// enough to tell two prompts apart. A runner with no block markers
/// — or none mapped — yields no block.
#[test]
fn prompt_block_on_codex_and_others() {
    let codex_a = "some output\nWould you like to run `rm -rf build`?\n› 1. Yes, proceed\n";
    let codex_b = "some output\nWould you like to run `cargo test`?\n› 1. Yes, proceed\n";
    assert_ne!(
        prompt_block("codex", codex_a).expect("codex block"),
        prompt_block("codex", codex_b).unwrap()
    );
    assert_eq!(prompt_block("codex", "plain output\n"), None);
    for runner in ["opencode", "unknown"] {
        assert_eq!(
            prompt_block(runner, "Permission required\nAllow once\n"),
            None,
            "{runner}"
        );
    }
}

/// A Devin prompt screen as read on 3000.12.32: the `●` block holds
/// the command, `▲ Approval required` introduces the judge line and
/// the `❭ Allow` picker carries its scope after the arrows.
fn devin_screen(command: &str, judge: &str, scope: &str) -> String {
    format!(
        " Thinking · 2s (esc twice to interrupt)\n\
         \u{0020}● Running command\n\
         │ $ {command}\n\
         ▲ Approval required\n\
         \u{0020} {judge}\n\
         ❭ Allow ← ◼◼◼◼ → {scope}     Exec(flock)\n\
         \u{0020} Deny\n\
         ↑↓ select · ←→ 1-4 scope · e edit · ↵ confirm · esc cancel\n"
    )
}

/// Devin's block starts at the `●` line above `Approval required`,
/// so the command is inside it — two commands differ, and so do the
/// judge's sentence and the scope word. A spinner line above `●`
/// stays out of the block.
#[test]
fn prompt_block_on_devin() {
    let a = devin_screen(
        "cd /home/user/acme-shop && cargo test -p shop",
        "This action requires your approval.",
        "Once",
    );
    let block = prompt_block("devin", &a).expect("devin block");
    assert!(block.contains("cargo test -p shop"), "{block}");
    assert!(!block.contains("esc twice to interrupt"), "{block}");
    assert_ne!(
        block,
        prompt_block(
            "devin",
            &devin_screen(
                "flock /tmp/x.lock make",
                "This action requires your approval.",
                "Once",
            ),
        )
        .unwrap(),
        "a different command must be a different block"
    );
    assert_ne!(
        block,
        prompt_block(
            "devin",
            &devin_screen(
                "cd /home/user/acme-shop && cargo test -p shop",
                "This writes outside the workspace.",
                "Once",
            ),
        )
        .unwrap(),
        "a different judge line must be a different block"
    );
    assert_ne!(
        block,
        prompt_block(
            "devin",
            &devin_screen(
                "cd /home/user/acme-shop && cargo test -p shop",
                "This action requires your approval.",
                "Session",
            ),
        )
        .unwrap(),
        "a different scope must be a different block"
    );
    assert_eq!(prompt_block("devin", "plain output\n"), None);
    // No `●` above `Approval required` — nothing comparable.
    assert_eq!(
        prompt_block("devin", "▲ Approval required\n❭ Allow ← ◼ → Once\n  Deny\n"),
        None
    );
}

fn approval(seq: u64, role: &str, by: &str, decision: ApprovalDecision) -> ApprovalRecord {
    ApprovalRecord {
        seq,
        role: role.into(),
        by: by.into(),
        decision,
        ts_ms: 1,
    }
}

/// A `deny` strands the role at its interrupted screen, so it is
/// owed one notice naming who denied it — an actor's points at
/// `backlot block`, a non-actor's at reporting back, since `block`
/// is actor-only; `allow` owes nothing, and a carried `source_seq`
/// owes nothing again.
#[test]
fn deny_notice_owed_once() {
    let mut state = StudioState::default();
    state.approvals.push(approval(
        4,
        "actor-t1-c1",
        "director",
        ApprovalDecision::Deny,
    ));
    state
        .approvals
        .push(approval(6, "writer", "user", ApprovalDecision::Deny));
    state.approvals.push(approval(
        8,
        "actor-t2-c1",
        "director",
        ApprovalDecision::Allow,
    ));
    let dir = Path::new("/state/studios/acme-shop");
    let got = notices::owed(&state, dir);
    let denied: Vec<_> = got.iter().filter(|n| n.label == "denied").collect();
    assert_eq!(denied.len(), 2, "{got:?}");
    let actor = denied.iter().find(|n| n.source_seq == 4).unwrap();
    assert_eq!(actor.to, "actor-t1-c1");
    assert!(actor.body.contains("director"), "{}", actor.body);
    assert!(actor.body.contains("did not run"), "{}", actor.body);
    assert!(
        actor.body.contains("backlot block --reason"),
        "{}",
        actor.body
    );
    let writer = denied.iter().find(|n| n.source_seq == 6).unwrap();
    assert_eq!(writer.to, "writer");
    assert!(writer.body.contains("user"), "{}", writer.body);
    assert!(
        writer.body.contains("needs no permission"),
        "{}",
        writer.body
    );
    assert!(
        writer
            .body
            .contains("tell whoever asked what could not be done"),
        "{}",
        writer.body
    );
    assert!(
        !writer.body.contains("backlot block"),
        "non-actor deny must not name the actor-only block: {}",
        writer.body
    );
    assert!(got.iter().all(|n| n.source_seq != 8));
    // Once a message carries that source_seq, nothing is owed.
    state.messages.push(backlot_model::state::Message {
        seq: 9,
        from: "stage".into(),
        to: "actor-t1-c1".into(),
        file: Some("messages/4-denied.md".into()),
        purpose: None,
        source_seq: Some(4),
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    });
    assert!(notices::owed(&state, dir).iter().all(|n| n.source_seq != 4));
}

//! Fail-first tests for `<hdir>/handoffs/<id>.md` brief construction,
//! mode 0600 permissions, 32 KiB / 4 KiB caps, unanswered-tell rescue,
//! and probation prompt formatting.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use backlot_model::hollywood::{CardAction, CardBase, HollywoodKind, HollywoodState, apply};
use backlot_model::state::StudioState;
use backlot_model::testkit::hev;

use crate::handoff_pkg::{
    HandoffBriefInput, MAX_BRIEF_BYTES, MAX_MEMORY_BYTES, build_handoff_brief,
    candidate_first_prompt, write_handoff_brief,
};
use crate::live::LiveView;
use crate::test_support::TestDir;
use crate::transcript::{ExtractedTail, TailKind};

#[test]
fn builds_and_writes_0600_handoff_brief_with_cards_queued_unanswered_and_memory() {
    let tmp = TestDir::new("bl-hpkg");
    let hdir = tmp.path().join("hollywood");
    fs::create_dir_all(&hdir).unwrap();
    fs::write(hdir.join("memory.md"), "# Memory\nKeep beta-api paused.\n").unwrap();

    let mut hstate = HollywoodState::default();
    // Card c1 stays open
    apply(
        &mut hstate,
        &hev(
            1,
            "hollywood",
            HollywoodKind::CardProposed {
                action: CardAction::Pause {
                    studio: "acme-shop".into(),
                    task: "t1".into(),
                },
                why: "actor looping".into(),
                base: CardBase::TaskStatus {
                    was: backlot_model::state::TaskStatus::Running,
                },
            },
        ),
    );
    // User tell #2 was delivered, with NO subsequent reply/card -> must be flagged unanswered!
    apply(
        &mut hstate,
        &hev(
            2,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("Check oauth bug before merging".into()),
                file: None,
                source_seq: None,
            },
        ),
    );
    apply(
        &mut hstate,
        &hev(3, "stage", HollywoodKind::MessageDelivered { target: 2 }),
    );
    // User tell #4 is still queued (undelivered)
    apply(
        &mut hstate,
        &hev(
            4,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("Please summarize acme-shop".into()),
                file: None,
                source_seq: None,
            },
        ),
    );

    let studios = BTreeMap::<String, StudioState>::new();
    let live = LiveView::default();
    let tail = ExtractedTail {
        kind: TailKind::RunnerSession,
        provenance: "claude session sid-1".into(),
        markdown: "### User\nhi\n\n### Assistant\nchecked status\n".into(),
    };
    let brief = build_handoff_brief(&HandoffBriefInput {
        id: "h-0001",
        hdir: &hdir,
        now_ms: 10_000,
        from_actor: "claude/sonnet/high",
        to_actor: "codex/gpt-6-sol/high",
        trigger: "quota exhausted",
        studios: &studios,
        live: &live,
        hstate: &hstate,
        tail: &tail,
    });
    assert!(brief.len() <= MAX_BRIEF_BYTES);
    assert!(brief.contains("h-0001"));
    assert!(brief.contains("from: `claude/sonnet/high`"));
    assert!(brief.contains("to: `codex/gpt-6-sol/high`"));
    assert!(brief.contains("quota exhausted"));
    assert!(brief.contains("c1"));
    assert!(brief.contains("Check oauth bug before merging"));
    assert!(brief.contains("sent to previous agent without a reply"));
    assert!(brief.contains("Please summarize acme-shop"));
    assert!(brief.contains("queued — stage will deliver after handoff commits"));
    assert!(brief.contains("Keep beta-api paused."));
    assert!(brief.contains("checked status"));

    let path = write_handoff_brief(&hdir, "h-0001", &brief).unwrap();
    assert_eq!(path, hdir.join("handoffs/h-0001.md"));
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let prompt = candidate_first_prompt(&hdir, "h-0001", "claude", "sonnet", "codex", "gpt-6-sol");
    assert!(prompt.contains("handoffs/h-0001.md"));
    assert!(prompt.contains("backlot hollywood report"));
    assert!(prompt.contains("Do NOT propose cards or tell studio producers"));
}

#[test]
fn enforces_4_kib_memory_cap_and_32_kib_total_brief_cap() {
    let tmp = TestDir::new("bl-hpkg-cap");
    let hdir = tmp.path().join("hollywood");
    fs::create_dir_all(&hdir).unwrap();
    let mem = format!("OLD_MEM {}\nNEW_MEM_MARKER\n", "a".repeat(8000));
    fs::write(hdir.join("memory.md"), mem).unwrap();

    let hstate = HollywoodState::default();
    let studios = BTreeMap::new();
    let live = LiveView::default();
    let tail = ExtractedTail {
        kind: TailKind::TerminalFallback,
        provenance: "terminal scrollback + ledger exchange".into(),
        markdown: "x\n".repeat(16_000),
    };
    let brief = build_handoff_brief(&HandoffBriefInput {
        id: "h-cap",
        hdir: &hdir,
        now_ms: 20_000,
        from_actor: "claude/sonnet",
        to_actor: "codex/gpt-6-sol",
        trigger: "user switch",
        studios: &studios,
        live: &live,
        hstate: &hstate,
        tail: &tail,
    });
    assert!(brief.len() <= MAX_BRIEF_BYTES, "len = {}", brief.len());
    assert!(brief.contains("NEW_MEM_MARKER"));
    assert!(!brief.contains(&"a".repeat(MAX_MEMORY_BYTES + 200)));
}

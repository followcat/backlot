//! The footer's slot mask against the real dispatch: for every
//! target the bar can select, in both agent states, a slot is lit
//! exactly when the key's dispatch runs it — the footer never
//! advertises a key the row would eat and never hides one it runs
//! (S1 §5). `i tell` is the gate's own case: the tell line dies
//! without `[hollywood]`, so the footer must not name it there.

use super::*;

use crossterm::event::KeyCode;

/// The row keys the footer can name, with the words a lit slot
/// shows in the full tier.
const ROW_KEYS: [(char, &str); 7] = [
    ('y', "y/d answer"),
    ('d', "y/d answer"),
    ('a', "a/x card"),
    ('x', "a/x card"),
    ('f', "f finish"),
    ('s', "s start"),
    ('p', "p peek"),
];

/// A plain press of `ch` — the shape the layer's dispatch reads.
fn press_of(ch: char) -> crate::app::input::TerminalKey {
    crate::app::input::TerminalKey::new(hc_vt::KeyInput {
        key: hc_vt::Key::Char(ch),
        mods: hc_vt::Mods::NONE,
        action: hc_vt::Action::Press,
    })
}

/// The footer for the selected row at a width that always takes the
/// full tier — the same call the renderer makes.
fn footer_of(app: &App, agent: bool) -> String {
    let machines = app.panel_summaries();
    let status = crate::hollywood::status_of(&app.view);
    let waits = crate::hollywood::waiting_items(&machines, &status);
    crate::hollywood::for_row(
        app.hollywood.selected.as_ref(),
        app.hollywood.editing,
        &waits,
        agent,
        196,
    )
}

/// The base document plus the variants that put a user-owned
/// prompt, a ready studio, a calm band, a down stage and an
/// unreachable machine on the sheet — each in both agent states.
fn fixtures(agent: bool) -> Vec<(&'static str, App)> {
    let path = std::path::Path::new("config.toml");
    let read = |agent| if agent { Ok(Some(cfg(None))) } else { Ok(None) };
    let mut out: Vec<(&'static str, App)> = Vec::new();
    // The base: frames, the headline and two cards, the tail row.
    let mut app = app_with_doc();
    app.set_hollywood(read(agent), path);
    out.push(("doc", app));
    // The producer's prompt user-owned: the sheet's prompt row.
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["attention_user"] = true.into();
    value["studios"][0]["roles"][0]["attention_seq"] = 7.into();
    value["studios"][0]["roles"][0]["attention_detail"] = "may I run the tests?".into();
    out.push(("prompt", with_doc(value, read(agent), path)));
    // beta-tools ready to finish: the READY row and its frame.
    let mut value = summary_json();
    value["studios"][1]["ready_to_finish"] = true.into();
    out.push(("ready", with_doc(value, read(agent), path)));
    // No cards and no user prompts: the calm row.
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    out.push(("calm", with_doc(value, read(agent), path)));
    // The stage down: the Stage row `s` restarts.
    let mut app = app_with_doc();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .map(|mut t| {
            if t.name.as_deref() == Some("backlot-stage") {
                t.state = "exited".into();
            }
            t
        })
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.set_hollywood(read(agent), path);
    out.push(("stage-down", app));
    // host-b's document with no machine row: unreachable, its
    // studios live — the offline Stage row `s` never runs on.
    let mut app = app_with_doc();
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    app.set_hollywood(read(agent), path);
    out.push(("offline", app));
    out
}

/// An app settled over `value`'s document and configured per `read`.
fn with_doc(
    value: serde_json::Value,
    read: Result<Option<HollywoodCfg>, String>,
    path: &std::path::Path,
) -> App {
    let mut app = app_with_doc();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.set_hollywood(read, path);
    app
}

/// For every target, in both agent states, a slot is lit exactly
/// when the key's dispatch runs — the slot table may not drift from
/// `hollywood_row_key`, and `i` follows the layer's own gate.
#[tokio::test]
async fn the_footer_names_exactly_the_keys_the_dispatch_runs() {
    for agent in [true, false] {
        for (name, mut app) in fixtures(agent) {
            let (recorded, runner) = recorder();
            app.set_hollywood_runner(runner);
            app.area = (200, 40);
            for target in app.hollywood_targets() {
                app.hollywood.mode = LayerMode::Expanded;
                app.hollywood.selected = Some(target.clone());
                let footer = footer_of(&app, agent);
                for (ch, token) in ROW_KEYS {
                    // Each press measured on its own: an earlier
                    // key's run still pends (no reply is pumped) and
                    // a same-mark spawn dedups before the runner
                    // sees argv — the slot names what this press
                    // would dispatch, not what a pending mark swallows.
                    app.actions.pending.clear();
                    let before = recorded.lock().unwrap().len();
                    let mut ran = app.hollywood_row_key(KeyCode::Char(ch));
                    if ch == 's' {
                        // `s` is dual-use — on a card it runs `card
                        // show`, which the `s start` slot does not
                        // name. The slot follows the stage dispatch
                        // this press made, or did not make.
                        ran = recorded.lock().unwrap()[before..]
                            .iter()
                            .any(|a| a.first().is_some_and(|w| w == "stage"));
                    }
                    assert_eq!(
                        footer.contains(token),
                        ran,
                        "{name} agent={agent} {target:?} '{ch}': footer `{footer}`"
                    );
                }
                // `i` rides the layer's own dispatch — its gate sits
                // above the row keys.
                app.hollywood.view = None;
                app.hollywood.confirm = None;
                app.hollywood.editing = false;
                app.studio_dialog = None;
                app.handle_hollywood_key(&press_of('i'));
                let tell = app.hollywood.editing;
                app.hollywood.editing = false;
                assert_eq!(
                    footer.contains("i tell"),
                    tell,
                    "{name} agent={agent} {target:?} 'i': footer `{footer}`"
                );
            }
        }
    }
}

/// The drawn footer follows the gate: the no-agent bar's Setup row
/// names nothing but the block, the configured one's input row
/// names the tell.
#[tokio::test]
async fn the_drawn_footer_follows_the_tell_gate() {
    let path = std::path::Path::new("config.toml");
    let mut app = app_with_doc();
    app.set_hollywood(Ok(None), path);
    app.area = (200, 40);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(Target::Setup);
    let text = screen_lines(&draw_to(&mut app, 200, 40)).join("\n");
    assert!(text.contains("↑↓ ⏎"), "the bare block: {text}");
    assert!(!text.contains("i tell"), "no tell without an agent: {text}");
    app.set_hollywood(Ok(Some(cfg(None))), path);
    app.hollywood.selected = Some(Target::Input);
    let text = screen_lines(&draw_to(&mut app, 200, 40)).join("\n");
    assert!(text.contains("i tell"), "the tell with an agent: {text}");
}

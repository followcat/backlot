//! Fake-core pane tests: a real `Attach` over duplex wires, the scripted
//! `testcore` server, and direct `Pane` driving. No real sessions.

use super::*;
use crate::testcore::{self, FakeCore};
use hc_proto::frame::Kind;
use hc_proto::terminal::AttachParams;
use hc_vt::{Action, Key, KeyInput, Mods};
use serde_json::json;
use tokio::time::timeout;

const CH: u32 = 7;
const T: std::time::Duration = testcore::DEADLINE;

fn spec(role: Role) -> Spec {
    Spec {
        terminal: "t7".into(),
        role,
    }
}

fn key(c: char) -> crate::app::input::TerminalKey {
    crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Char(c),
        mods: Mods::NONE,
        action: Action::Press,
    })
}

fn rig() -> (
    Pane,
    mpsc::Receiver<PaneEv>,
    mpsc::Receiver<(FakeCore, AttachParams)>,
) {
    let (ev_tx, ev_rx) = mpsc::channel(64);
    let (acc_tx, acc_rx) = mpsc::channel(16);
    let open: OpenAttach = Arc::new(move |params: AttachParams| {
        let acc = acc_tx.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            let p2 = params.clone();
            acc.send((core, params)).await.expect("attach accept");
            Attach::open(read, write, None, &p2).await
        })
    });
    (Pane::new(ev_tx, open), ev_rx, acc_rx)
}

async fn step(pane: &mut Pane, rx: &mut mpsc::Receiver<PaneEv>) {
    let ev = timeout(T, rx.recv())
        .await
        .expect("pane event deadline")
        .expect("pane event channel");
    pane.on_event(ev, Instant::now());
}

async fn serve(
    accept: &mut mpsc::Receiver<(FakeCore, AttachParams)>,
    negotiated: bool,
) -> FakeCore {
    let (mut core, _params) = timeout(T, accept.recv())
        .await
        .expect("accept deadline")
        .expect("accept");
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        negotiated,
    ))
    .await;
    core
}

async fn step_until_live(pane: &mut Pane, rx: &mut mpsc::Receiver<PaneEv>) {
    for _ in 0..8 {
        if !pane.status_label(Instant::now()).contains("attaching") {
            return;
        }
        step(pane, rx).await;
    }
    panic!("pane never reached Live");
}

async fn paint(
    pane: &mut Pane,
    rx: &mut mpsc::Receiver<PaneEv>,
    core: &mut FakeCore,
    bytes: &[u8],
) {
    let mut payload = bytes.to_vec();
    if matches!(&pane.state, State::Live(live) if live.await_paint || live.repaint.is_some()) {
        payload.extend_from_slice(b"\x1b[?9001l");
    }
    raw_paint(pane, rx, core, &payload).await;
}

async fn raw_paint(
    pane: &mut Pane,
    rx: &mut mpsc::Receiver<PaneEv>,
    core: &mut FakeCore,
    bytes: &[u8],
) {
    core.send_stream(CH, Kind::Data, bytes).await;
    timeout(T, async {
        loop {
            let event = rx.recv().await.expect("pane event channel");
            let painted = matches!(&event,
                PaneEv::Out { generation, out: Output::Data(data) }
                if *generation == pane.generation && data.as_slice() == bytes);
            pane.on_event(event, Instant::now());
            if painted {
                break;
            }
        }
    })
    .await
    .expect("paint event deadline");
}

async fn read_kind(core: &mut FakeCore, ch: u32, kind: Kind) -> hc_proto::frame::Frame {
    timeout(T, async {
        loop {
            let f = core.read_stream(ch).await;
            if f.kind == kind {
                return f;
            }
        }
    })
    .await
    .expect("frame kind deadline")
}

async fn no_kind(core: &mut FakeCore, ch: u32, kinds: &[Kind], ms: u64) {
    let deadline = Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        match timeout(deadline - Instant::now(), core.read_stream(ch)).await {
            Ok(f) => assert!(!kinds.contains(&f.kind), "unexpected frame {f:?}"),
            Err(_) => return,
        }
    }
}

fn vt_text(pane: &mut Pane) -> String {
    let Some(vt) = pane.vt() else {
        return String::new();
    };
    vt.frame(0)
        .unwrap()
        .rows
        .iter()
        .flat_map(|r| r.cells.iter().map(|c| c.text.as_str()))
        .collect()
}

fn view_with(
    pid: Option<u32>,
    reason: Option<&str>,
    cols: u16,
    rows: u16,
) -> hc_proto::watch::View {
    let mut term = crate::sidebar::fixture::term("t7");
    term.pid = pid;
    term.reason = reason.map(str::to_string);
    term.cols = cols;
    term.rows = rows;
    let mut view = hc_proto::watch::View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
        }),
        machines: Some(vec![]),
        ..Default::default()
    };
    view.inventory.replace(1, vec![term]);
    view
}

fn live_view(cols: u16, rows: u16) -> hc_proto::watch::View {
    view_with(Some(100), None, cols, rows)
}

fn release(c: char) -> crate::app::input::TerminalKey {
    crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Char(c),
        mods: Mods::NONE,
        action: Action::Release,
    })
}

#[path = "host_tests.rs"]
mod host_tests;

#[path = "input_bounds_tests.rs"]
mod input_bounds_tests;

#[path = "close_tests.rs"]
mod close_tests;

#[path = "tests_parts/cases_1.rs"]
mod cases_1;

#[path = "tests_parts/cases_2.rs"]
mod cases_2;

#[path = "recovery_tests.rs"]
mod recovery_tests;

#[path = "generation_tests.rs"]
mod generation_tests;

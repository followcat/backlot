//! Neutral test fixtures: core-shaped records built field by field.
//! No real hosts, users or sessions — `host-a`/`host-b`, `alice`, and
//! `/home/user/...` stand in everywhere.

use hc_proto::hello::CoreInfo;
use hc_proto::machine::MachineView;
use hc_proto::terminal::{Status, Terminal};
use hc_proto::watch::{Link, View};

/// A minimal live shell terminal: pid set, never reported.
pub fn term(address: &str) -> Terminal {
    Terminal {
        address: address.into(),
        name: None,
        labels: Default::default(),
        argv: vec!["sh".into()],
        cwd: "/home/user/proj".into(),
        keep: false,
        state: "live".into(),
        pid: Some(100),
        exit_code: None,
        signal: None,
        reason: None,
        created_ms: 0,
        finished_ms: None,
        screen: "exact".into(),
        cols: 80,
        rows: 24,
        identity: String::new(),
        status: None,
        status_ms: None,
        records: Default::default(),
        turn: 0,
        reported: false,
        waiting_for_person: false,
        title: String::new(),
        parent: None,
        notify: false,
        die_with_parent: false,
        rm: false,
        template: None,
        first_message: None,
        submit: None,
        stop: None,
        paste_gap_ms: None,
        foreground: None,
        extra: Default::default(),
    }
}

/// A 7501 record carrying `word` (plus optional kind/progress/source).
pub fn status(word: &str) -> Status {
    Status {
        state: word.into(),
        ..Status::default()
    }
}

/// A terminal that reported `word` as its root record.
pub fn agent(address: &str, word: &str) -> Terminal {
    let mut t = term(address);
    t.reported = true;
    t.status = Some(status(word));
    t
}

/// A connected machine view as `machine.list` reports it.
pub fn machine(name: &str, state: &str) -> MachineView {
    MachineView {
        name: name.into(),
        target: format!("{name}-ssh"),
        id: format!("id-{name}"),
        host: name.into(),
        version: "0.1.0".into(),
        state: state.into(),
        session: None,
        since_ms: None,
        temporary: false,
        via: false,
        why: None,
    }
}

/// A view on core `host-a`, online, with `machines` listed and
/// `terminals` installed as the epoch-1 snapshot.
pub fn view(terminals: Vec<Terminal>, machines: Vec<MachineView>) -> View {
    let mut view = View {
        link: Link::Online,
        rpc: Link::Online,
        core: Some(CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
        }),
        machines: Some(machines),
        ..View::default()
    };
    view.inventory.replace(1, terminals);
    view
}

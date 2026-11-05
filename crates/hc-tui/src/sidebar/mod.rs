//! Shared sidebar helpers the projected chrome consumes: `status`
//! resolves each terminal's effective status record (root + named) and
//! maps it to a word; `text` sanitizes and fits display text; `fixture`
//! builds minimal terminal records for tests.
//!
//! The old `Sidebar` row model and its ratatui renderer were removed
//! once `chrome::render_shell` became the authoritative visual path —
//! semantics now live in `crate::project` and the ported `chrome`
//! renderers.

pub mod status;
mod text;

pub(crate) use status::{Life, Word, classify, effective};
pub(crate) use text::clean;

/// Minimal core-shaped records for tests; tweak fields as needed.
/// Mirrors `tests/common` for unit tests inside the crate — the
/// integration copy cannot be shared (`cfg(test)` code is not
/// compiled into the lib the tests link against).
#[cfg(test)]
pub(crate) mod fixture {
    use hc_proto::hello::CoreInfo;
    use hc_proto::machine::MachineView;
    use hc_proto::terminal::{Status, Terminal};
    use hc_proto::watch::{Link, View};

    pub(crate) fn term(address: &str) -> Terminal {
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
            repository: None,
            activity: None,
            extra: Default::default(),
        }
    }

    /// A 7501 record carrying `word`.
    pub(crate) fn status(word: &str) -> Status {
        Status {
            state: word.into(),
            ..Status::default()
        }
    }

    /// A terminal that reported `word` as its root record.
    pub(crate) fn agent(address: &str, word: &str) -> Terminal {
        let mut t = term(address);
        t.reported = true;
        t.status = Some(status(word));
        t
    }

    /// A `machine.list` row.
    pub(crate) fn machine(name: &str, state: &str) -> MachineView {
        MachineView {
            name: name.into(),
            target: format!("{name}-ssh"),
            id: format!("id-{name}"),
            host: name.into(),
            hostname: None,
            version: "0.1.0".into(),
            state: state.into(),
            session: None,
            since_ms: None,
            temporary: false,
            via: false,
            why: None,
        }
    }

    /// A healthy view: online lanes, `host-a` core identity.
    pub(crate) fn view(terms: Vec<Terminal>, machines: Vec<MachineView>) -> View {
        let mut view = View {
            link: Link::Online,
            rpc: Link::Online,
            core: Some(CoreInfo {
                id: "id-host-a".into(),
                name: "host-a".into(),
                hostname: None,
            }),
            machines: Some(machines),
            ..Default::default()
        };
        view.inventory.replace(1, terms);
        view
    }
}

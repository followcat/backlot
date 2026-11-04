//! User-facing text for the `hc-tui` command-line interface.
//!
//! Milestone M0 is a bootstrap build: it prints status information only and
//! never opens a connection to `hc`.

/// Version line printed for `--version`.
pub fn version_text() -> String {
    format!("hc-tui {}", env!("CARGO_PKG_VERSION"))
}

/// Long help printed for `--help`.
pub fn help_text() -> String {
    format!(
        "\
hc-tui {}
Terminal UI for herdr-core (hc). Milestone M0 bootstrap build.

The interactive TUI is not implemented yet: hc-tui prints a status
message and exits. It does not connect to any hc session.

Usage: hc-tui [OPTIONS]

Options:
  -h, --help     Print this help
  -V, --version  Print the version",
        env!("CARGO_PKG_VERSION"),
    )
}

/// Status message printed when hc-tui runs without arguments.
pub fn bootstrap_text() -> String {
    format!(
        "\
hc-tui {} — milestone M0 bootstrap
Protocol generation: {} (via hc-proto)

The terminal UI is not implemented yet. No connection to hc was
attempted. Milestone M1 adds the protocol client; see docs/plan.md.",
        env!("CARGO_PKG_VERSION"),
        hc_proto::PROTOCOL_GENERATION,
    )
}

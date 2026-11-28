use std::process::ExitCode;

use hc_tui::cli::{Action, Options, help_text, parse, run_options, upgrade, version_text};
use hc_tui::restart::exec_installed;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Studio commands are handled before any TUI parsing.
    if let Some(code) = backlot_stage::maybe_run(&args) {
        return code;
    }
    match parse(args) {
        Ok(Action::Help) => {
            println!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("{}", version_text());
            ExitCode::SUCCESS
        }
        Ok(Action::Upgrade) => upgrade(),
        Ok(Action::Run(options)) => run_client(&options),
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Try 'backlot --help' for more information.");
            ExitCode::from(2)
        }
    }
}

/// Runs the client. A restart execs the installed binary only after the
/// runtime is gone and `run_options` has restored the terminal.
fn run_client(options: &Options) -> ExitCode {
    // A new client version refreshes the agent skill copies backlot
    // installed; a missing manifest means nothing to do.
    let _ = backlot_stage::skill::refresh(&backlot_stage::cli::Env::process());
    let outcome = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(run_options(
            options,
            &hc_proto::endpoint::env_snapshot(),
            &hc_proto::endpoint::Sys,
        )),
        Err(e) => {
            eprintln!("error: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(bin)) => {
            let error = exec_installed(&bin, std::env::args_os().skip(1));
            eprintln!("error: restart: {}: {error}", bin.display());
            ExitCode::FAILURE
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

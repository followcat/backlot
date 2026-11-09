use std::process::ExitCode;

use hc_tui::cli::{Action, help_text, parse, run_options, version_text};

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Action::Help) => {
            println!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Action::Version) => {
            println!("{}", version_text());
            ExitCode::SUCCESS
        }
        Ok(Action::Run(options)) => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(rt) => match rt.block_on(run_options(
                    &options,
                    &hc_proto::endpoint::env_snapshot(),
                    &hc_proto::endpoint::Sys,
                )) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(message) => {
                        eprintln!("error: {message}");
                        ExitCode::FAILURE
                    }
                },
                Err(e) => {
                    eprintln!("error: runtime: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Try 'backlot --help' for more information.");
            ExitCode::from(2)
        }
    }
}

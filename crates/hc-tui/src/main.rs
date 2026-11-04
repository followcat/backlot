use std::process::ExitCode;

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Try 'hc-tui --help' for more information.");
            ExitCode::from(2)
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<String, String> {
    match args.next().as_deref() {
        None => Ok(hc_tui::bootstrap_text()),
        Some("-h" | "--help") => Ok(hc_tui::help_text()),
        Some("-V" | "--version") => Ok(hc_tui::version_text()),
        Some(other) => Err(format!("unexpected argument '{other}'")),
    }
}

//! Whether a `type = "popup"` command line just starts an interactive
//! shell — the default for an unset `persistent` key. A popup that only
//! spawns a shell is a persistent per-tab float; every other command
//! stays an ephemeral popup unless it sets `persistent = true`.

/// Program basenames that mean "interactive shell" when invoked with no
/// non-option arguments.
const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "nu", "ksh", "dash", "ash", "csh", "tcsh", "elvish", "xonsh",
    "pwsh",
];

/// `true` when `command` is a bare interactive-shell invocation: an
/// optional leading `exec`, then exactly one non-option word that names a
/// shell or expands `$SHELL`/`${SHELL…}`, plus only `-`-leading options.
/// Conservative by design — anything ambiguous stays ephemeral.
pub(crate) fn popup_command_is_shell(command: &str) -> bool {
    let mut words = command.split_whitespace().peekable();
    if words.peek() == Some(&"exec") {
        words.next();
    }
    let mut program = None;
    for word in words {
        let word = word.trim_matches(|c| c == '\'' || c == '"');
        if word.starts_with('-') {
            continue;
        }
        if program.is_some() {
            return false;
        }
        program = Some(word);
    }
    let Some(program) = program else {
        return false;
    };
    if program.contains("$SHELL") || program.contains("${SHELL") {
        return true;
    }
    let basename = program.rsplit('/').next().unwrap_or(program);
    SHELLS.contains(&basename)
}

#[cfg(test)]
mod tests {
    use super::popup_command_is_shell;

    #[test]
    fn documented_alt_f_example_is_a_shell() {
        assert!(popup_command_is_shell("exec \"${SHELL:-sh}\""));
        assert!(popup_command_is_shell("exec $SHELL"));
        assert!(popup_command_is_shell("exec \"${SHELL}\""));
    }

    #[test]
    fn plain_shells_and_options_are_shells() {
        for command in [
            "sh",
            "exec bash",
            "bash",
            "zsh -l",
            "exec zsh --login",
            "fish -i",
            "/bin/bash",
            "/usr/bin/nu",
            "exec /bin/sh -l",
        ] {
            assert!(popup_command_is_shell(command), "{command}");
        }
    }

    #[test]
    fn non_shell_commands_are_not_shells() {
        for command in [
            "echo hi",
            "htop",
            "tmux",
            "tmux attach",
            "exec sh -c 'echo hi'",
            "exec sh script.sh",
            "sudo -i",
            "exec bash && exit",
            "env FOO=1 bash",
            "make build",
            "exec",
            "",
            "   ",
        ] {
            assert!(!popup_command_is_shell(command), "{command}");
        }
    }
}

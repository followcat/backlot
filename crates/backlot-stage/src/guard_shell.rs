//! Shell command-line parsing for hook-based outward-action guards.

use crate::guard_rules::refused_command;

/// Inspect a shell command line and return the formatted refused command
/// if any command segment invokes an outward action refused by `guard_rules`.
pub(crate) fn refused_command_line(line: &str) -> Option<String> {
    refused_line_depth(line, 0)
}

fn refused_line_depth(line: &str, depth: usize) -> Option<String> {
    split_shell_segments(line)
        .into_iter()
        .find_map(|seg| eval_segment(&seg, depth))
}

fn eval_segment(seg: &[String], depth: usize) -> Option<String> {
    let mut i = 0;
    while i < seg.len() {
        let tok = seg[i].as_str();
        if is_env_assign(tok) {
            i += 1;
            continue;
        }
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if base == "env" {
            i += 1;
            while i < seg.len() {
                let flag = seg[i].as_str();
                if flag == "--" {
                    i += 1;
                    break;
                }
                if matches!(flag, "-u" | "-C" | "-S" | "--unset" | "--chdir") {
                    i += 2;
                } else if flag.starts_with('-') {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if tok == "command" {
            i += 1;
            if seg
                .get(i)
                .is_some_and(|f| f.starts_with("-v") || f.starts_with("-V"))
            {
                return None;
            }
            continue;
        }
        if matches!(tok, "exec" | "nohup" | "time") {
            i += 1;
            continue;
        }
        break;
    }
    let cmd = seg.get(i)?;
    let base = cmd.rsplit('/').next().unwrap_or(cmd);
    let rest = &seg[i + 1..];
    match base {
        "sh" | "bash" | "zsh" | "dash" | "ash" if depth < 4 => eval_nested_shell(rest, depth + 1),
        "git" => eval_git(rest),
        _ => refused_command(base, rest),
    }
}

fn eval_nested_shell(rest: &[String], next_depth: usize) -> Option<String> {
    let mut j = 0;
    while j < rest.len() {
        let arg = rest[j].as_str();
        if arg == "--" || !arg.starts_with('-') {
            break;
        }
        if arg == "-o" || arg == "+o" {
            j += 2;
            continue;
        }
        if !arg.starts_with("--")
            && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
            && arg.contains('c')
        {
            return rest
                .get(j + 1)
                .and_then(|script| refused_line_depth(script, next_depth));
        }
        j += 1;
    }
    None
}

fn eval_git(rest: &[String]) -> Option<String> {
    let mut j = 0;
    while j < rest.len() {
        let arg = rest[j].as_str();
        if arg == "--" {
            return None;
        }
        if matches!(
            arg,
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--exec-path"
                | "--super-prefix"
                | "--config-env"
        ) {
            j += 2;
            continue;
        }
        if arg.starts_with('-') {
            j += 1;
            continue;
        }
        break;
    }
    if rest.get(j).map(String::as_str) != Some("push") {
        return None;
    }
    let positional: Vec<String> = rest[j + 1..]
        .iter()
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();
    refused_command("git-push", &positional)
}

fn is_env_assign(tok: &str) -> bool {
    let Some((name, _)) = tok.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn split_shell_segments(line: &str) -> Vec<Vec<String>> {
    let mut segs = Vec::new();
    let mut seg = Vec::new();
    let mut word = String::new();
    let mut has_word = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut chars = line.chars().peekable();

    let flush_word = |seg: &mut Vec<String>, word: &mut String, has_word: &mut bool| {
        if *has_word {
            seg.push(std::mem::take(word));
            *has_word = false;
        }
    };
    let flush_seg = |segs: &mut Vec<Vec<String>>, seg: &mut Vec<String>| {
        if !seg.is_empty() {
            segs.push(std::mem::take(seg));
        }
    };

    while let Some(c) = chars.next() {
        if escape {
            escape = false;
            if c != '\n' {
                word.push(c);
                has_word = true;
            }
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                word.push(c);
                has_word = true;
            }
            continue;
        }
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => {
                    word.push(c);
                    has_word = true;
                }
            }
            continue;
        }
        match c {
            '#' if !has_word => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            '\'' => {
                in_single = true;
                has_word = true;
            }
            '"' => {
                in_double = true;
                has_word = true;
            }
            '\\' => escape = true,
            '\n' | ';' | '(' | ')' => {
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut seg);
            }
            '&' | '|' => {
                if chars.peek() == Some(&c) {
                    chars.next();
                }
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut seg);
            }
            _ if c.is_whitespace() => {
                flush_word(&mut seg, &mut word, &mut has_word);
            }
            _ => {
                word.push(c);
                has_word = true;
            }
        }
    }
    flush_word(&mut seg, &mut word, &mut has_word);
    flush_seg(&mut segs, &mut seg);
    segs
}

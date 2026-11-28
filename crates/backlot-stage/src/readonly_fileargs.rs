//! Filesystem-operand extraction for the more structured read-only
//! programs: find, the pattern readers, jq, tar and git.

use std::path::{Path, PathBuf};

use crate::readonly_args::{check_all, named_file_values, positionals, short_cluster_has};
use crate::readonly_path::obvious_path;

pub(super) fn check_find(
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    if args.iter().any(|a| a == "-fls") {
        return Err("find -fls writes a file");
    }
    let mut paths = Vec::new();
    let mut i = 0;
    // GNU find's global options precede the path list. `-D` and `-O` carry
    // their values in the next word or the same word; skipping them keeps
    // `find -L <path> …` from losing `<path>` entirely.
    while i < args.len() {
        match args[i].as_str() {
            "-H" | "-L" | "-P" => i += 1,
            "-D" if i + 1 < args.len() => i += 2,
            a if a.starts_with("-O") => i += 1,
            _ => break,
        }
    }
    while i < args.len() && !args[i].starts_with('-') && args[i] != "(" && args[i] != "!" {
        paths.push(args[i].clone());
        i += 1;
    }
    while i < args.len() {
        if (args[i] == "-newer" || args[i] == "-samefile" || args[i].starts_with("-newer"))
            && i + 1 < args.len()
        {
            paths.push(args[i + 1].clone());
            i += 1;
        }
        i += 1;
    }
    check_all(paths, cwd, roots, home)
}

pub(super) fn check_pattern_reader(
    prog: &str,
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    if matches!(prog, "rg" | "grep")
        && args.iter().any(|a| {
            (short_cluster_has(a, 'e') && a != "-e") || (short_cluster_has(a, 'f') && a != "-f")
        })
    {
        return Err("a clustered pattern or pattern-file option cannot be checked");
    }
    let file_flags: &[&str] = if prog == "fd" {
        &["--search-path", "--base-directory"]
    } else {
        &["-f", "--file"]
    };
    let mut paths = named_file_values(args, file_flags);
    let value_flags: &[&str] = match prog {
        "rg" => &[
            "-e",
            "--regexp",
            "-f",
            "--file",
            "-g",
            "--glob",
            "--iglob",
            "-t",
            "--type",
            "-T",
            "--type-not",
            "--type-add",
            "--sort",
            "--sortr",
            "--colors",
            "--replace",
            "-m",
            "--max-count",
            "-A",
            "-B",
            "-C",
            "--context",
            "--max-depth",
            "--max-filesize",
            "--path-separator",
            "--context-separator",
            "--field-context-separator",
            "--field-match-separator",
            "-j",
            "--threads",
        ],
        "grep" => &[
            "-e",
            "--regexp",
            "-f",
            "--file",
            "-m",
            "--max-count",
            "-A",
            "-B",
            "-C",
            "--context",
            "-d",
            "--directories",
            "-D",
            "--devices",
            "--include",
            "--exclude",
            "--exclude-dir",
            "--label",
            "--binary-files",
            "--group-separator",
            "--color",
            "--colors",
        ],
        "fd" => &[
            "-e",
            "--extension",
            "-t",
            "--type",
            "-E",
            "--exclude",
            "-d",
            "--max-depth",
            "--min-depth",
            "-S",
            "--size",
            "--changed-within",
            "--changed-before",
            "-j",
            "--threads",
            "--max-results",
            "--owner",
            "--and",
            "--search-path",
            "--base-directory",
        ],
        _ => &[],
    };
    let positional = positionals(args, value_flags);
    let pattern_supplied = match prog {
        "rg" => args.iter().any(|a| {
            a == "--files"
                || a == "--type-list"
                || a == "-e"
                || a == "--regexp"
                || a.starts_with("--regexp=")
                || a == "-f"
                || a == "--file"
                || a.starts_with("--file=")
                || a.starts_with("--generate")
        }),
        "grep" => args.iter().any(|a| {
            a == "-e"
                || a == "--regexp"
                || a.starts_with("--regexp=")
                || a == "-f"
                || a == "--file"
                || a.starts_with("--file=")
        }),
        _ => false,
    };
    let start = if pattern_supplied {
        0
    } else {
        1.min(positional.len())
    };
    paths.extend(positional.into_iter().skip(start));
    check_all(paths, cwd, roots, home)
}

pub(super) fn check_jq(
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    let mut paths = named_file_values(args, &["-f", "--from-file", "-L", "--library-path"]);
    let mut i = 0;
    while i < args.len() {
        if matches!(args[i].as_str(), "--argfile" | "--slurpfile" | "--rawfile") {
            if i + 2 >= args.len() {
                return Err("jq file-valued option is incomplete");
            }
            paths.push(args[i + 2].clone());
            i += 3;
            continue;
        }
        i += 1;
    }
    let positional = positionals(
        args,
        &[
            "-f",
            "--from-file",
            "-L",
            "--library-path",
            "--arg",
            "--argjson",
            "--indent",
            "--tab",
            "--stream-indent",
            "--run-tests",
            "--args",
            "--jsonargs",
        ],
    );
    let script_file = args
        .iter()
        .any(|a| a == "-f" || a == "--from-file" || a.starts_with("--from-file="));
    paths.extend(positional.into_iter().skip(usize::from(!script_file)));
    check_all(paths, cwd, roots, home)
}

pub(super) fn check_tar(
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    let Some(mode) = args.first() else {
        return Err("tar without a mode");
    };
    let list = if mode == "--list" {
        true
    } else if mode.starts_with("--") {
        false
    } else {
        let cluster = mode.trim_start_matches('-');
        cluster.contains('t')
            && !cluster
                .chars()
                .any(|c| matches!(c, 'c' | 'x' | 'r' | 'u' | 'd' | 'A' | 'I' | 'F'))
    };
    if !list {
        return Err("only tar's list mode is read-only");
    }
    if args.iter().any(|a| {
        a == "--index-file"
            || a.starts_with("--index-file=")
            || a == "--rsh-command"
            || a.starts_with("--rsh-command=")
            || a == "--rmt-command"
            || a.starts_with("--rmt-command=")
    }) {
        return Err("tar output or remote-command options are not read-only");
    }
    // In dashed clusters `f` and `C` take the rest of their own word when
    // anything follows them. Refuse that form; the separated spelling is the
    // one this checker can prove.
    if args.iter().any(|a| {
        a.starts_with('-')
            && !a.starts_with("--")
            && a[1..]
                .chars()
                .enumerate()
                .any(|(n, c)| matches!(c, 'f' | 'C') && n + 1 < a.len() - 1)
    }) {
        return Err("an attached tar file or directory value cannot be checked");
    }
    let mut paths = named_file_values(args, &["-f", "--file", "-C", "--directory"]);
    let value_flags = &[
        "-f",
        "--file",
        "-C",
        "--directory",
        "--exclude",
        "--transform",
        "--strip-components",
        "--occurrence",
        "--index-file",
        "--exclude-tag",
        "--exclude-tag-all",
        "--exclude-tag-under",
    ];
    paths.extend(
        positionals(args, value_flags)
            .into_iter()
            .skip(usize::from(!mode.starts_with('-'))),
    );
    if paths.iter().any(|p| p != "-" && p.contains(':')) {
        return Err("a colon in a tar operand can select a remote archive");
    }
    check_all(paths, cwd, roots, home)
}

pub(super) fn check_git(
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    let mut paths = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-C" | "--git-dir" | "--work-tree" => {
                if i + 1 >= args.len() {
                    return Err("git location option is incomplete");
                }
                paths.push(args[i + 1].clone());
                i += 2;
            }
            "-c" | "--config-env" => return Err("git config overrides are not read-only-checked"),
            "--no-pager"
            | "--literal-pathspecs"
            | "--glob-pathspecs"
            | "--noglob-pathspecs"
            | "--icase-pathspecs"
            | "--no-optional-locks" => i += 1,
            _ if a.starts_with("--git-dir=") || a.starts_with("--work-tree=") => {
                paths.push(
                    a.split_once('=')
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_default(),
                );
                i += 1;
            }
            _ if a.starts_with('-') => return Err("unrecognised git global option"),
            _ => break,
        }
    }
    let Some(sub) = args.get(i).map(String::as_str) else {
        return Err("git without a read subcommand");
    };
    if !matches!(
        sub,
        "log"
            | "show"
            | "diff"
            | "status"
            | "branch"
            | "rev-parse"
            | "ls-files"
            | "blame"
            | "describe"
    ) {
        return Err("not a read-only git subcommand");
    }
    let rest = &args[i + 1..];
    if sub == "branch" {
        if !rest.iter().any(|a| a == "--list" || a == "-l") {
            return Err("git branch is read-only only with --list");
        }
        if rest.iter().any(|a| {
            matches!(
                a.as_str(),
                "-d" | "-D"
                    | "-m"
                    | "-M"
                    | "-c"
                    | "-C"
                    | "-u"
                    | "--unset-upstream"
                    | "--set-upstream-to"
                    | "--edit-description"
                    | "--move"
                    | "--copy"
                    | "--delete"
            )
        }) {
            return Err("git branch mutation flags are not read-only");
        }
    }
    let mut after_dashdash = false;
    for a in rest {
        if *a == "--" {
            after_dashdash = true;
            continue;
        }
        if obvious_path(a) || (after_dashdash && !a.starts_with('-')) {
            paths.push(a.clone());
        }
    }
    check_all(paths, cwd, roots, home)
}

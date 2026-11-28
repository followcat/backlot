//! Program-specific argument validation for the read-only classifier.

use std::path::{Path, PathBuf};

use crate::readonly_path::{check_path, obvious_path};

pub(super) fn short_cluster_has(arg: &str, flag: char) -> bool {
    arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(flag)
}

pub(super) fn long_value(arg: &str, name: &str) -> Option<String> {
    arg.strip_prefix(name)
        .and_then(|v| v.strip_prefix('='))
        .map(str::to_string)
}

pub(super) fn takes_value(args: &[String], i: usize, names: &[&str]) -> bool {
    names.contains(&args[i].as_str()) && i + 1 < args.len()
}

pub(super) fn positionals(args: &[String], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut plain = false;
    while i < args.len() {
        let a = &args[i];
        if plain {
            out.push(a.clone());
            i += 1;
            continue;
        }
        if a == "--" {
            plain = true;
            i += 1;
            continue;
        }
        if takes_value(args, i, value_flags) {
            i += 2;
            continue;
        }
        if !a.starts_with('-') || a == "-" {
            out.push(a.clone());
        }
        i += 1;
    }
    out
}

pub(super) fn check_all(
    paths: impl IntoIterator<Item = String>,
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    for path in paths {
        if path != "-" {
            check_path(&path, cwd, roots, home)?;
        }
    }
    Ok(())
}

pub(super) fn check_obvious(
    args: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    for arg in args {
        if obvious_path(arg) {
            check_path(arg, cwd, roots, home)?;
        }
    }
    Ok(())
}

pub(super) fn named_file_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if names.contains(&a) && i + 1 < args.len() {
            out.push(args[i + 1].clone());
            i += 2;
            continue;
        }
        for name in names {
            if name.starts_with("--")
                && let Some(v) = long_value(a, name)
            {
                out.push(v);
            }
        }
        i += 1;
    }
    out
}

pub(crate) fn check_program(
    words: &[String],
    cwd: &Path,
    roots: &[PathBuf],
    home: Option<&Path>,
) -> Result<(), &'static str> {
    let prog = words[0].as_str();
    if prog.is_empty() || prog.contains(['/', '\\']) {
        return Err("the program word is not a bare name");
    }
    if !matches!(
        prog,
        "ls" | "cat"
            | "head"
            | "tail"
            | "wc"
            | "rg"
            | "grep"
            | "fd"
            | "find"
            | "du"
            | "df"
            | "which"
            | "test"
            | "["
            | "stat"
            | "file"
            | "tree"
            | "sha256sum"
            | "jq"
            | "sort"
            | "uniq"
            | "cut"
            | "tr"
            | "echo"
            | "printf"
            | "nproc"
            | "free"
            | "ps"
            | "tar"
            | "git"
    ) {
        return Err("program is not in the read-only set");
    }
    let args = &words[1..];
    if crate::permit_rules::exec_action(prog, args) {
        return Err("an argument can execute or write");
    }
    match prog {
        "find" => crate::readonly_fileargs::check_find(args, cwd, roots, home),
        "rg" | "grep" | "fd" => {
            crate::readonly_fileargs::check_pattern_reader(prog, args, cwd, roots, home)
        }
        "jq" => crate::readonly_fileargs::check_jq(args, cwd, roots, home),
        "tar" => crate::readonly_fileargs::check_tar(args, cwd, roots, home),
        "git" => crate::readonly_fileargs::check_git(args, cwd, roots, home),
        "sort" => {
            if args.iter().any(|a| {
                a == "-T"
                    || a == "--temporary-directory"
                    || a.starts_with("--temporary-directory=")
                    || a == "--files0-from"
                    || a.starts_with("--files0-from=")
            }) {
                return Err("sort temporary or indirect file lists are not read-only-checked");
            }
            let mut paths = named_file_values(args, &["--files0-from", "--random-source"]);
            paths.extend(positionals(
                args,
                &[
                    "-k",
                    "--key",
                    "-t",
                    "--field-separator",
                    "-S",
                    "--buffer-size",
                    "--batch-size",
                    "--parallel",
                    "--files0-from",
                    "--random-source",
                ],
            ));
            check_all(paths, cwd, roots, home)
        }
        "uniq" => {
            let positional = positionals(
                args,
                &[
                    "-c",
                    "--count",
                    "-f",
                    "--skip-fields",
                    "-s",
                    "--skip-chars",
                    "-w",
                    "--check-chars",
                ],
            );
            if positional.len() > 1 {
                return Err("uniq's second file operand is an output");
            }
            check_all(positional, cwd, roots, home)
        }
        "tree" => {
            if args.iter().any(|a| {
                a == "-o"
                    || a == "--output"
                    || a.starts_with("--output=")
                    || a == "--fromfile"
                    || a.starts_with("--fromfile=")
            }) {
                return Err("tree output or indirect file lists are not read-only-checked");
            }
            let mut paths = named_file_values(args, &["--fromfile"]);
            paths.extend(positionals(
                args,
                &[
                    "-L",
                    "-P",
                    "-I",
                    "--filelimit",
                    "--timefmt",
                    "--sort",
                    "--charset",
                    "--fromfile",
                ],
            ));
            check_all(paths, cwd, roots, home)
        }
        "file" => {
            if args
                .iter()
                .any(|a| a == "-f" || a == "--files-from" || a.starts_with("--files-from="))
            {
                return Err("file's indirect file list is not read-only-checked");
            }
            let mut paths = named_file_values(args, &["-m", "--magic-file", "-f", "--files-from"]);
            paths.extend(positionals(
                args,
                &[
                    "-m",
                    "--magic-file",
                    "-f",
                    "--files-from",
                    "-F",
                    "--separator",
                ],
            ));
            check_all(paths, cwd, roots, home)
        }
        "sha256sum" => {
            if args
                .iter()
                .any(|a| a == "-c" || a == "--check" || short_cluster_has(a, 'c'))
            {
                return Err("checksum files name indirect paths");
            }
            let positional = positionals(args, &[]);
            check_all(positional, cwd, roots, home)
        }
        "ls" | "cat" | "head" | "tail" | "wc" | "du" | "df" | "stat" | "cut" => {
            let positional = positionals(
                args,
                &[
                    "-n",
                    "--lines",
                    "-c",
                    "--bytes",
                    "-m",
                    "--chars",
                    "-b",
                    "--bits",
                    "-f",
                    "--fields",
                    "-d",
                    "--delimiter",
                    "--output-delimiter",
                    "--time-style",
                    "--sort",
                    "--block-size",
                    "--format",
                    "--quoting-style",
                    "-I",
                    "--ignore",
                    "--hide",
                    "-T",
                    "--tabsize",
                    "-w",
                    "--width",
                    "--printf",
                    "-t",
                    "--total",
                    "--exclude",
                    "--max-depth",
                    "-B",
                    "--scale",
                    "-H",
                    "--si",
                ],
            );
            check_all(positional, cwd, roots, home)
        }
        "test" | "[" => {
            let positional = positionals(args, &[]);
            check_all(
                positional
                    .into_iter()
                    .filter(|a| a != "]")
                    .collect::<Vec<_>>(),
                cwd,
                roots,
                home,
            )
        }
        "which" | "echo" | "printf" | "nproc" | "free" | "ps" | "tr" => {
            check_obvious(args, cwd, roots, home)
        }
        _ => Err("program is not in the read-only set"),
    }
}

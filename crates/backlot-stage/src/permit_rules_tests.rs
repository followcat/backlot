use crate::permit_rules::{Derivation, derive};

fn rules(cmd: &str) -> Vec<(String, bool)> {
    match derive(cmd) {
        Derivation::Rules(rs) => rs.into_iter().map(|r| (r.rule, r.outward)).collect(),
        Derivation::OnceOnly(why) => panic!("once-only: {why}"),
    }
}

fn once(cmd: &str) -> String {
    match derive(cmd) {
        Derivation::OnceOnly(why) => why,
        Derivation::Rules(rs) => panic!("grantable: {rs:?}"),
    }
}

#[test]
fn design_shapes_derive_table_driven() {
    let cases: &[(&str, &[(&str, bool)])] = &[
        (
            "du -sh /home/user/acme-shop 2>/dev/null; ls ~",
            &[("du -sh", false), ("ls", false)],
        ),
        (
            "git -C /home/user/acme-shop log --oneline",
            &[("git -C /home/user/acme-shop log", false)],
        ),
        ("tar tzf /home/user/archive.tgz", &[("tar tzf", false)]),
        (
            "fd . verification -t f | sed -n '1,160p'",
            &[("fd", false), ("sed -n", false)],
        ),
        (
            "curl -s \"https://example.test/data\"",
            &[("curl -s", true)],
        ),
        ("npm run docs:build", &[("npm run docs:build", false)]),
        ("pip install wheel", &[("pip install", true)]),
        ("uv pip install wheel", &[("uv pip install", true)]),
        (
            "git clone https://example.test/repo.git",
            &[("git clone", true)],
        ),
        ("ssh host-a uptime", &[("ssh", true)]),
    ];
    for (cmd, want) in cases {
        let got = rules(cmd);
        let wantv: Vec<(String, bool)> = want.iter().map(|(r, o)| (r.to_string(), *o)).collect();
        assert_eq!(got, wantv, "{cmd}");
    }
}

#[test]
fn once_only_shapes_name_the_reason() {
    for (cmd, part) in [
        (
            "curl -s \"https://example.test\" | python3 -c \"print(1)\"",
            "arbitrary code",
        ),
        ("for f in *.rs; do wc -l $f; done", "loops"),
        ("python3 script.py", "arbitrary code"),
        ("rm -rf build", "destructive"),
        ("git reset --hard HEAD", "never remembered"),
        ("fd . -x rm {}", "exec or delete"),
        ("sh -c 'ls'", "arbitrary code"),
    ] {
        assert!(once(cmd).contains(part), "{} -> {}", cmd, once(cmd));
    }
}

#[test]
fn program_words_stay_exactly_as_typed() {
    for (cmd, want) in [
        ("./ls -la", "./ls -la"),
        ("/tmp/x/head -n 1 f", "/tmp/x/head -n"),
        ("~/bin/rg pattern", "~/bin/rg"),
        ("/usr/bin/git status", "/usr/bin/git status"),
    ] {
        assert_eq!(rules(cmd), vec![(want.into(), false)], "{cmd}");
    }
    assert!(!crate::permit_cmd::fixed_covers("./ls"));
    assert!(!crate::permit_cmd::fixed_covers("/tmp/x/head"));
    assert!(crate::permit_cmd::fixed_covers("ls"));
    assert!(crate::permit_cmd::fixed_covers("head"));
}

#[test]
fn interpreters_in_disguise_are_never_grantable() {
    for prog in [
        "awk",
        "gawk",
        "mawk",
        "nawk",
        "npx",
        "pnpx",
        "bunx",
        "uvx",
        "bun",
        "deno",
        "php",
        "lua",
        "Rscript",
        "osascript",
    ] {
        let cmd = format!("{prog} --version");
        assert!(once(&cmd).contains("arbitrary code"), "{cmd}");
    }
    assert!(once("/bin/rm -rf build").contains("destructive"));
}

#[test]
fn multi_command_families_need_safe_subcommands() {
    for (cmd, want) in [
        ("go test ./...", "go test"),
        ("make test", "make test"),
        ("just check", "just check"),
        ("poetry check", "poetry check"),
        ("pnpm test", "pnpm test"),
        ("yarn test", "yarn test"),
    ] {
        assert_eq!(rules(cmd), vec![(want.into(), false)], "{cmd}");
    }
    for cmd in [
        "uv run tool",
        "uv tool run x",
        "npm exec x",
        "npm x x",
        "pnpm exec x",
        "pnpm dlx x",
        "yarn exec x",
        "yarn dlx x",
    ] {
        assert!(
            once(cmd).contains("arbitrary code"),
            "{cmd} -> {}",
            once(cmd)
        );
    }
}

#[test]
fn exec_and_write_capable_flags_are_once_only_wherever_they_appear() {
    for cmd in [
        "find . -type f -exec rm {} ;",
        "find . -type f -fprint /tmp/out",
        "fd . -t f --exec rm",
        "rg foo --pre ./script",
        "sort input -o output",
        "sort input --compress-program gzip",
        "tar tzf a.tgz --to-command='sh -c x'",
        "tar tzf a.tgz --checkpoint-action=exec=x",
        "sed -i s/a/b/ f",
        "sed -n 'e ls' f",
        "git diff --output=/tmp/out",
        "git diff --ext-diff",
    ] {
        assert!(
            once(cmd).contains("exec or delete"),
            "{cmd} -> {}",
            once(cmd)
        );
    }
}

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

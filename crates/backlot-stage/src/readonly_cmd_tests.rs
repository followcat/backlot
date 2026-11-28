//! Table-driven tests for the provably read-only command classifier.

use std::fs;
use std::path::PathBuf;

use crate::readonly_cmd::{check_command, roots};
use crate::test_support::TestDir;

struct Fx {
    _dir: TestDir,
    cwd: PathBuf,
    roots: Vec<PathBuf>,
    home: PathBuf,
}

fn fixture() -> Fx {
    let dir = TestDir::new("backlot-readonly-test");
    let studio = dir.join("studio");
    let repo = dir.join("repo");
    let role = studio.join("roles/actor-t1-c1");
    let work = studio.join("work/t1");
    for d in [&studio, &repo, &role, &work] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(work.join("f.txt"), "x\n").unwrap();
    fs::write(repo.join("repo.txt"), "x\n").unwrap();
    fs::write(work.join("archive.tar"), b"").unwrap();
    Fx {
        roots: roots(&studio, &repo, &role, &work),
        cwd: work,
        home: dir.join("home"),
        _dir: dir,
    }
}

fn ok(fx: &Fx, line: &str) {
    assert!(
        check_command(line, &fx.cwd, &fx.roots, Some(&fx.home)).is_ok(),
        "want read-only: {line}"
    );
}

fn no(fx: &Fx, line: &str) {
    assert!(
        check_command(line, &fx.cwd, &fx.roots, Some(&fx.home)).is_err(),
        "want refused: {line}"
    );
}

#[test]
fn plain_read_only_programs_pass() {
    let fx = fixture();
    for line in [
        "ls",
        "ls -la .",
        "cat f.txt",
        "head -n 5 f.txt",
        "tail -n 5 f.txt",
        "wc -l f.txt",
        "rg needle",
        "rg needle f.txt",
        "grep needle f.txt",
        "fd needle",
        "find . -type f",
        "find -L . -type f",
        "du .",
        "df .",
        "which ls",
        "test -f f.txt",
        "[ -f f.txt ]",
        "stat f.txt",
        "file f.txt",
        "tree .",
        "sha256sum f.txt",
        "jq . f.txt",
        "sort f.txt",
        "uniq f.txt",
        "cut -c1 f.txt",
        "tr a b",
        "echo hello",
        "printf hello",
        "nproc",
        "free",
        "ps",
        "tar tf archive.tar",
        "git log",
        "git show",
        "git diff",
        "git status",
        "git branch --list",
        "git rev-parse HEAD",
        "git ls-files",
        "git blame f.txt",
        "git describe",
        "git -C . status",
        "ls . | wc -l",
        "cat f.txt >/dev/null",
        "cat f.txt >>/dev/null",
        "cat f.txt >> /dev/null",
        "cat f.txt 2>/dev/null",
        "cat f.txt 2>&1",
    ] {
        ok(&fx, line);
    }
}

#[test]
fn exec_and_write_capable_flags_refuse() {
    let fx = fixture();
    for line in [
        "rg --pre cat needle",
        "fd -x echo",
        "fd --exec echo",
        "find . -exec echo x \\;",
        "find . -delete",
        "find . -fprint out",
        "find . -fls out",
        "sort -o out f.txt",
        "sort --compress-program gzip f.txt",
        "sort -T . f.txt",
        "sort --files0-from=list",
        "sha256sum -c sums",
        "file -f list",
        "tree --fromfile .",
        "tar tf archive.tar --to-command cat",
        "tar -Itf archive.tar",
        "git diff --ext-diff",
        "git show --output=out",
        "git -c x=y status",
        "tree -o out .",
        "uniq f.txt out",
        "sed -n 1p f.txt",
        "rg -eneedle f.txt",
        "tar -tfarchive.tar",
        "tar tf archive.tar --index-file out",
        "git --paginate log",
        "ps e",
        "ps auxe",
        "ps eww",
        "ps -o pid,env",
        "ps -o pid,env:8",
        "ps -o pid,environment",
        "ps -oenviron",
        "ps o pid,environ",
        "jq -n env",
        "jq -n '$ENV'",
        "jq 'env.X'",
    ] {
        no(&fx, line);
    }
}

#[test]
fn shell_shape_escape_hatches_refuse() {
    let fx = fixture();
    for line in [
        "./ls",
        "/bin/ls",
        "cat f.txt > out",
        "cat f.txt>out",
        "cat f.txt < input",
        "cat f.txt | tee out",
        "echo $(cat f.txt)",
        "echo `cat f.txt`",
        "cat <(printf x)",
        "cat f.txt &",
        "A=1 ls",
        "cat <<EOF\nx\nEOF",
        "unknown",
        "env ls",
        "echo $OPENAI_API_KEY",
        "printf '%s' \"$TOKEN\"",
        "cat ${HOME}/.zshrc",
        "cat {f.txt,/etc/passwd}",
        "cat ~other/.zshrc",
    ] {
        no(&fx, line);
    }
}

#[test]
fn paths_stay_inside_roots_and_secrets_refuse() {
    let fx = fixture();
    no(&fx, "cat /etc/passwd");
    no(&fx, "find -L /etc -type f");
    no(&fx, "cat ../../../../etc/passwd");
    fs::write(fx.cwd.join(".env"), "x").unwrap();
    no(&fx, "cat .env");
    let secret = fx.cwd.join(".ssh");
    fs::create_dir_all(&secret).unwrap();
    fs::write(secret.join("id"), "x").unwrap();
    no(&fx, "cat .ssh/id");
    no(&fx, "cat .ssh/*");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", fx.cwd.join("outlink")).unwrap();
        no(&fx, "cat outlink");
    }
    ok(&fx, "cat missing-new-file");
}

#[test]
fn globs_expand_and_every_match_stays_inside_roots() {
    let fx = fixture();
    fs::write(fx.cwd.join("linka"), "x").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", fx.cwd.join("linkout")).unwrap();
        no(&fx, "cat link*");
        fs::write(fx.cwd.join("qa"), "x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", fx.cwd.join("qb")).unwrap();
        no(&fx, "cat q?");
        no(&fx, "cat q[ab]");
        fs::remove_file(fx.cwd.join("qb")).unwrap();
    }
    fs::remove_file(fx.cwd.join("linkout")).unwrap();
    ok(&fx, "cat link*");
    ok(&fx, "cat q?");
    ok(&fx, "cat q[ab]");
    // A glob with no match is still checked as the literal word.
    ok(&fx, "cat no-such-*");
}

#[test]
fn relative_paths_resolve_against_the_real_cwd() {
    let fx = fixture();
    let repo = fx.roots[1].clone();
    assert!(
        check_command("cat ../escape", &repo, &fx.roots, Some(&fx.home)).is_err(),
        "the same relative spelling must escape from the repo cwd"
    );
    assert!(
        check_command("cat ../escape", &fx.cwd, &fx.roots, Some(&fx.home)).is_ok(),
        "from the role work area it stays inside the studio root"
    );
}

fn actor() -> backlot_model::ActorIdentity {
    backlot_model::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    }
}

#[test]
fn auto_answer_coverage_accepts_readonly_and_refuses_secret() {
    let dir = TestDir::new("backlot-readonly-auto-test");
    let studio_dir = dir.join("studio");
    let repo = dir.join("repo");
    let work = studio_dir.join("work/integration");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    let mut state = backlot_model::state::StudioState::default();
    state.studio = Some(backlot_model::state::StudioConfig {
        title: "acme".into(),
        repo: repo.to_string_lossy().into_owned(),
        members: Vec::new(),
        no_git: false,
        mode: Default::default(),
        roles: backlot_model::event::RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        },
        candidates: Vec::new(),
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
        design_gate: backlot_model::event::DesignGate::Off,
    });
    let mut env = crate::cli::Env::default();
    env.vars.insert(
        "HOME".into(),
        dir.join("home").to_string_lossy().into_owned(),
    );
    assert!(crate::permit_auto::readonly_shell(
        &state,
        "director",
        &studio_dir,
        "ls",
        Some(&work),
        &env,
    ));
    assert!(!crate::permit_auto::readonly_shell(
        &state,
        "director",
        &studio_dir,
        "ls",
        None,
        &env,
    ));
    assert!(!crate::permit_auto::readonly_shell(
        &state,
        "director",
        &studio_dir,
        "ls",
        Some(std::path::Path::new("/")),
        &env,
    ));
    // `repo` is itself a root, but the relative path escapes it. Using
    // the role work area instead would wrongly keep the path inside
    // the studio directory.
    assert!(!crate::permit_auto::readonly_shell(
        &state,
        "director",
        &studio_dir,
        "cat ../escape",
        Some(&repo),
        &env,
    ));
    std::fs::write(work.join(".env"), "x").unwrap();
    assert!(!crate::permit_auto::readonly_shell(
        &state,
        "director",
        &studio_dir,
        "cat .env",
        Some(&work),
        &env,
    ));
}

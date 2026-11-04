# Instructions for coding agents

Read `docs/requirements.md` and `docs/plan.md` first. Work on one milestone task at a time.

- **English** everywhere: code, comments, commit messages, docs.
- **Small commits** with Conventional Commit subjects (`feat(sidebar): …`, `fix(proto): …`), each
  with its tests.
- **Before every commit**, all three must pass:
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
- **Never touch real hc sessions.** Tests and manual runs use a scratch session with temporary XDG
  directories (see `docs/plan.md` › Testing rules). Do not connect to `default` or any named
  session that you did not create.
- **No private data** in code, fixtures, logs or docs: no real host names, user names, home paths,
  addresses, tokens or emails. Use `host-a`, `alice`, `/home/user`.
- **Ported code:** code taken from herdr or herdr-core starts with a comment naming the source file
  and the Apache-2.0 license, for example:
  ```rust
  // Derived from herdr src/selection.rs (Apache-2.0).
  ```
  Add the project to NOTICE if it is not there yet.
- **File size:** keep source files under about 400 lines. Split by responsibility.
- **No network access at run time** except the hc connection and the user's own SSH.

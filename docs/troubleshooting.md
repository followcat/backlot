# Troubleshooting

Real messages first, then what usually causes them. `hc-tui --version` prints the build version —
include it in reports.

## Installing

| Message | Cause and fix |
|---|---|
| `install.sh: no prebuilt hc-tui for FreeBSD/amd64` | Releases cover Linux x86_64/aarch64 (static musl) and macOS arm64/Intel. Build from source for anything else. |
| `install.sh: checksum mismatch for hc-tui-… — nothing was installed` | The archive did not match `sha256sums.txt`. Nothing was installed — re-run; if it persists, report it and do not use the file. |
| `install.sh: hc-tui-… is not listed in sha256sums.txt` | The release lacks your platform's archive, or `HC_TUI_BASE_URL`/`HC_TUI_VERSION` points somewhere odd. |
| `install.sh: could not determine the latest release tag` | The `releases/latest` redirect did not resolve (offline, or no release exists yet). Pin one with `HC_TUI_VERSION=v…`. |
| `install.sh: download failed — does release v… contain …` | Tag or archive missing — check the tag and platform spelling. |
| `install.sh: no sha256 tool found` | Install `sha256sum`, `shasum` or `openssl`. |
| `note: <dir> is not on your PATH` | Add it (e.g. `export PATH="$HOME/.local/bin:$PATH"`) or run `hc-tui` by its full path. |

## Starting

| Message / state | Cause and fix |
|---|---|
| `error: terminal setup: No such device or address (os error 6)` | hc-tui was launched without a terminal — piped, from cron, over `ssh` without `-t`. Run it in a real terminal. |
| `error: unexpected argument '…'` / `--session needs a value` | Command-line typo — `hc-tui --help` lists the flags. |
| `error: endpoint: session name "…" can't be a folder name` | `--session`/`HC_SESSION` may not contain a path separator. |
| Sidebar loads but every machine shows offline / `◐ reconnecting` | No core is answering on the resolved socket. Check `HC_SESSION`/`--session`, `HC_SOCKET`, and that `hc` is running (`hc server status`, `hc server start`). hc-tui keeps retrying — a started core appears on its own. |

## Remote (`--remote`)

| Message / state | Cause and fix |
|---|---|
| `ssh to host-a exited with exit status …: … command not found` | `hc` is missing from the remote's non-interactive PATH. Verify with `ssh host-a hc -V`; install herdr-core or fix the remote PATH. |
| `ssh to host-a exited with exit status 255` | The SSH layer itself failed — host unreachable, key rejected, host-key prompt refused (hc-tui runs ssh non-interactively). Get `ssh host-a` working in a normal shell first. |
| `cannot run ssh: …` | No `ssh` binary on the client machine's PATH. |
| `error: bad ssh target …` | `--remote` takes ssh-style targets only — no whitespace or control characters. |
| Attach/connect latency is one login per burst | The `$XDG_RUNTIME_DIR/hc-tui-ssh` control socket shares logins; where no safe runtime dir exists, each stream is its own login — still correct, just slower. |

## Inside the UI

- **Copied text never reaches the clipboard.** Selection copies through OSC 52 — the *outer*
  terminal (or tmux: `set -g set-clipboard on`) must allow it. Over SSH the terminal on your laptop
  is what matters, not the server.
- **Sidebar shows no Git repo/branch or agent statuses.** On official `hc` those facts don't exist;
  OSC 7501 agents still report status. See [compatibility.md](compatibility.md).
- **A pane shows a size/ownership notice.** Another client (e.g. herdr-app) owns that terminal's
  grid size — click the notice to take control.
- **Keys do nothing / everything types through.** `Ctrl+G` lock is on — every key passes to the
  terminal until `Ctrl+G` again. The tab bar shows the lock state.
- **Nothing reports bugs better than facts:** the version (`hc-tui --version`), your OS/terminal,
  the session, and the exact message or a screenshot.

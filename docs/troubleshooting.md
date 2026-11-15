# Troubleshooting

Common messages and what usually causes them. Messages are quoted exactly as hc-tui prints them.

## Installing

| Message | Cause and fix |
|---|---|
| `install.sh: no prebuilt hc-tui for FreeBSD/amd64` | There's no build for your platform. Build from source. |
| `install.sh: checksum mismatch for hc-tui-… — nothing was installed` | The download didn't match `sha256sums.txt`. Try again. If it keeps happening, don't use the file, and open an issue. |
| `install.sh: hc-tui-… is not listed in sha256sums.txt` | The release has no archive for your platform, or `HC_TUI_VERSION` or `HC_TUI_BASE_URL` is wrong. |
| `install.sh: could not determine the latest release tag` | You're offline, or there's no release yet. Set `HC_TUI_VERSION=v…` to pick one. |
| `install.sh: download failed — does release v… contain …` | The tag or the archive doesn't exist. Check the version. |
| `install.sh: no sha256 tool found` | Install `sha256sum`, `shasum` or `openssl`. |
| `note: <dir> is not on your PATH` | Add it, for example `export PATH="$HOME/.local/bin:$PATH"`, or run `hc-tui` by its full path. |

## Starting

hc-tui starts a core itself when the session is local and nothing answers on its socket: the socket file is
missing, or a stale one refuses the connection. It runs `hc server start` once and the usual reconnect picks the
core up. It never starts a core for `--remote`, and never when `HC_SOCKET` is set: a set `HC_SOCKET` names a
specific core that you manage yourself.

| Message or state | Cause and fix |
|---|---|
| `error: terminal setup: No such device or address (os error 6)` | hc-tui has no terminal: it was piped, run from cron, or started over `ssh` without `-t`. |
| `error: unexpected argument '…'` or `--session needs a value` | A typo on the command line. `hc-tui --help` lists the options. |
| `error: endpoint: session name "…" can't be a folder name` | `--session` and `HC_SESSION` can't contain a path separator. |
| `hc isn't installed` / `install it: curl -fsSL https://releases.herdr.dev/core/install.sh \| sh` | The socket answers nothing and there's no `hc` on `PATH`. Run the command the notice shows. |
| `Core start failed` / `<first error line>` | `hc server start` ran but failed. The notice shows the first line of its output; run `hc server start` in a shell to see the rest. |
| The sidebar loads but machines show offline or `◐ reconnecting` | No core is answering on the socket yet. If hc-tui started a core it appears on its own. For `--remote` or a set `HC_SOCKET`, start or fix that core yourself: `hc server status`, `hc server start`. |

## Remote

| Message or state | Cause and fix |
|---|---|
| `ssh to host-a exited with exit status …: … command not found` | `hc` isn't on the remote's non-interactive `PATH`. Check with `ssh host-a hc -V`. |
| `ssh to host-a exited with exit status 255` | SSH itself failed: the host is unreachable, the key was rejected, or it needs a prompt, and hc-tui turns prompts off. Get `ssh host-a` working in a normal shell first. |
| `cannot run ssh: …` | There's no `ssh` on this machine's `PATH`. |
| `error: bad ssh target …` | The `--remote` target can't contain spaces or control characters. |
| Every connection logs in separately | hc-tui couldn't create a private directory for its control socket (see [Remote use](remote.md)). Everything still works, but it's slower. |

## Inside the UI

- **Copied text doesn't reach the clipboard.** Selection copies through OSC 52, so the terminal you're looking at has
  to allow it. Over SSH that's the terminal on your laptop, not the server. In tmux, use `set -g set-clipboard on`.
- **A program says it copied, but your clipboard didn't change.** Codex, for one, writes the clipboard of the machine
  it runs on when that machine has a display, and sends the copy to your terminal (OSC 52) only when `HERDR_ENV=1`,
  SSH or tmux says something forwards it. Terminals hc-tui creates get `HERDR_ENV=1`. For terminals started another
  way, such as `hc run`, add this line to the shell's startup file (`~/.zshrc` or `~/.bashrc`) on that machine, then
  start a new shell and restart the program:

  ```sh
  [ -n "$HC_TERMINAL_ID" ] && export HERDR_ENV=1
  ```
- **The sidebar has no Git details or agent statuses.** Official `hc` doesn't provide Git details, and agents only show
  a status if they report it through OSC 7501. See [Core compatibility](compatibility.md).
- **A pane shows a size notice.** Another client, such as herdr-app, may own that terminal's size. Click the notice to
  take control.
- **`Ctrl+V` says `No image to paste` or `Image paste unavailable` though you copied an image.** The clipboard read
  needs `wl-paste` under Wayland or `xclip` under X11; install whichever your display uses. When both displays are
  reachable (Wayland with XWayland) both tools are tried, so an image only reachable through X11 still pastes.
  `The clipboard owner stopped answering` means a tool hit its deadline — the clipboard owner or the XWayland
  bridge is wedged. Check `wl-paste --list-types` and `xclip -selection clipboard -t TARGETS -o` by hand.
- **Shortcuts do nothing and every key goes to the program.** The keyboard lock is on. Press `Ctrl+G` to turn it off.
  The tab bar shows when it's on.
- **On macOS, `Ctrl+B q` doesn't exit.** Known issue. Automated tests on macOS see hc-tui keep running after this key
  in a pseudo-terminal. The cause isn't known yet, and it hasn't been checked in a real macOS terminal. If you hit it,
  close the terminal window: your agents keep running in the core. Please open an issue with your macOS version and
  terminal.

## Reporting a bug

Include the output of `hc-tui --version`, your OS and terminal, and the exact message or a screenshot.

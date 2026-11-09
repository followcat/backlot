# Install, upgrade, uninstall

hc-tui is one binary. It needs the `hc` core installed separately —
[herdr-core](https://github.com/herdrdev/herdr-core). Prebuilt archives:

| Platform | Archive target |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` (static) |
| Linux aarch64 | `aarch64-unknown-linux-musl` (static) |
| macOS arm64 | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |

## Install script

```sh
curl -fsSL https://github.com/followcat/hc-tui/releases/latest/download/install.sh | sh
```

The script (`scripts/install.sh`, plain POSIX sh):

1. detects OS and architecture and picks the matching archive — an unsupported platform gets a
   clear message instead of a guess;
2. resolves the latest release tag — `HC_TUI_VERSION=v0.1.0` pins one instead (a bare `0.1.0`
   works too);
3. downloads `hc-tui-<version>-<target>.tar.gz` and `sha256sums.txt` over HTTPS;
4. verifies the archive's checksum and stops with nothing installed on a mismatch;
5. writes the binary to `${HC_TUI_INSTALL_DIR:-$HOME/.local/bin}` via a rename inside that
   directory, so a running `hc-tui` is never half-replaced. No sudo.

It prints a note if the install dir is not on your `PATH`, and a reminder that `hc` must be
installed. `sha256` is computed with `sha256sum`, `shasum` or `openssl` — whichever the system has.

## Manual install

From the [Releases page](https://github.com/followcat/hc-tui/releases), download
`hc-tui-<version>-<target>.tar.gz` and `sha256sums.txt`, then:

```sh
sha256sum --check sha256sums.txt --ignore-missing   # or: shasum -a 256 -c sha256sums.txt
tar -xzf hc-tui-*-*.tar.gz
mv hc-tui ~/.local/bin/
```

The archive also contains `LICENSE`, `NOTICE` and a short `README.md`.

## Upgrade

Re-run the install script. `HC_TUI_VERSION=v<new>` moves to a specific release, up or down.
The binary is replaced atomically; running clients keep their own in-memory copy until restart.

## Uninstall

```sh
rm ~/.local/bin/hc-tui
```

hc-tui keeps its footprint small: your Herdr config is only written when you save a setting in
the Settings overlay (theme, status indicators, sound, toast delivery) — unrelated content is
preserved — and the SSH control socket under `$XDG_RUNTIME_DIR/hc-tui-ssh` (or
`/tmp/hc-tui-ssh-<uid>`) expires on its own — remove it too if you like. The `hc-tui` layout
document lives on the core, not here.

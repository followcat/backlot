# Install, upgrade, uninstall

backlot is one binary. It talks to the `hc` core: see
[herdr-core](https://github.com/herdrdev/herdr-core). The install script installs `hc` too when it is
missing. Prebuilt archives:

| Platform | Archive target |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` (static) |
| Linux aarch64 | `aarch64-unknown-linux-musl` (static) |
| macOS arm64 | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |

## Install script

```sh
curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | sh
```

The script (`scripts/install.sh`):

1. detects OS and architecture and picks the matching archive — an unsupported platform gets a
   clear message instead of a guess;
2. resolves the latest release tag — `BACKLOT_VERSION=v0.1.0` pins one instead (a bare `0.1.0`
   works too);
3. downloads `backlot-<version>-<target>.tar.gz` and `sha256sums.txt` over HTTPS;
4. verifies the archive's checksum and stops with nothing installed on a mismatch;
5. writes the binary to `${BACKLOT_INSTALL_DIR:-$HOME/.local/bin}` via a rename inside that
   directory, so a running `backlot` is never half-replaced. No sudo;
6. looks for `hc` on `PATH`, in the install dir and in `~/.local/bin`. When it's missing everywhere,
   the script runs the official core installer:
   `curl -fsSL https://releases.herdr.dev/core/install.sh | sh`. Set `BACKLOT_SKIP_CORE=1` to skip
   this step. If the core installer fails, backlot stays installed and the script prints the command
   to run later.

It tells you if the install directory isn't on your `PATH`. The checksum uses `sha256sum`, `shasum` or `openssl`,
whichever is available.

Once `hc` is installed you don't start it yourself: `backlot` starts a local core when it needs one. See
[troubleshooting](troubleshooting.md) for when it does and doesn't.

## Manual install

From the [Releases page](https://github.com/followcat/backlot/releases), download
`backlot-<version>-<target>.tar.gz` and `sha256sums.txt`, then:

```sh
sha256sum --check sha256sums.txt --ignore-missing   # or: shasum -a 256 -c sha256sums.txt
tar -xzf backlot-*-*.tar.gz
mv backlot ~/.local/bin/
```

The archive also contains `LICENSE`, `NOTICE` and a short `README.md`.

## Upgrade

Re-run the install script. `BACKLOT_VERSION=v<new>` moves to a specific release, up or down.
The binary is replaced atomically; running clients keep their own in-memory copy until restart.

## Uninstall

```sh
rm ~/.local/bin/backlot
```

backlot leaves little else behind:

- If you changed the theme, status indicators, sound or toast delivery in Settings, those are saved in your Herdr
  config. Other content in that file isn't touched.
- The SSH control socket lives in `$XDG_RUNTIME_DIR/hc-tui-ssh`, or `/tmp/hc-tui-ssh-<uid>`. It expires by itself, and
  you can delete the directory.
- The `hc-tui` layout document is stored on the core.

# Install, upgrade, uninstall

hc-tui is a single binary. It talks to the `hc` core: see
[herdr-core](https://github.com/herdrdev/herdr-core).

| Platform | Archive target |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` (static) |
| Linux aarch64 | `aarch64-unknown-linux-musl` (static) |
| macOS arm64 | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |

## Install script

```sh
curl -fsSL https://followcat.github.io/hc-tui/install.sh | sh
```

The same script is attached to every release, at
`https://github.com/followcat/hc-tui/releases/latest/download/install.sh`.

The script (`scripts/install.sh`):

1. picks the archive for your OS and architecture, and stops with a message if there isn't one;
2. finds the latest release, or uses the one in `HC_TUI_VERSION` (`v0.1.0` or `0.1.0`);
3. downloads `hc-tui-<version>-<target>.tar.gz` and `sha256sums.txt` over HTTPS;
4. checks the archive's checksum, and installs nothing if it doesn't match;
5. moves the binary into `${HC_TUI_INSTALL_DIR:-$HOME/.local/bin}`. It doesn't use sudo;
6. looks for `hc` on `PATH`, in the install dir and in `~/.local/bin`. When it's missing everywhere, the script
   runs the official core installer: `curl -fsSL https://releases.herdr.dev/core/install.sh | sh`. Set
   `HC_TUI_SKIP_CORE=1` to skip this step. If the core installer fails, hc-tui stays installed and the script
   prints the command to run later.

It tells you if the install directory isn't on your `PATH`. The checksum uses `sha256sum`, `shasum` or `openssl`,
whichever is available.

Once `hc` is installed you don't start it yourself: `hc-tui` starts a local core when it needs one. See
[troubleshooting](troubleshooting.md) for when it does and doesn't.

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

How you upgrade depends on how hc-tui was installed. `hc-tui --upgrade` checks: on a package-managed install or a
dev build it refuses before anything runs and prints the command to use instead. An explicit `HC_TUI_INSTALL_DIR` is
taken as your own choice and runs the installer as asked.

### Install script and manual installs

```sh
hc-tui --upgrade
```

`--upgrade` runs the install script again for the latest release, with its progress on your terminal, and asks you to
restart hc-tui. Set `HC_TUI_VERSION=v<version>` first to move to a specific release, newer or older. Clients that are
already running keep the old version until you restart them. `--upgrade` installs into the directory of the running
binary (`HC_TUI_INSTALL_DIR` overrides it), so the command upgrades the binary you run.

Inside the client, `prefix+u` (or a click on the `↑v…` badge) opens the update card with the release notes of every
newer release. `↵ update` runs the same installer locally, even when the client is attached to a remote host, checks
that `hc-tui --version` reports the new version, and then `↵ restart` re-executes the installed binary in place. The
card offers `↵ update` only when the install directory is writable; otherwise it shows the command to run by hand. On
a package-managed install the card never offers `↵ update` and names the package manager's own command instead.

### mise

```sh
mise upgrade --bump github:followcat/hc-tui
```

`--bump` rewrites an exact version pin; a plain `mise upgrade` stays inside the version the config asks for and can
do nothing.

### Homebrew

```sh
brew upgrade hc-tui
```

### Nix

Update hc-tui through the Nix configuration that installed it; there is no single upgrade command.

### cargo

Reinstall with cargo the way hc-tui was installed: build from source as the README shows
(`cargo build --release -p hc-tui`) and replace the binary under `$CARGO_HOME/bin` (`~/.cargo/bin` by default).

### Dev build

A binary under `target/` is a build artifact, not an install: rebuild it (`cargo build --release -p hc-tui`) and run
the new binary.

### Update check

On startup hc-tui asks GitHub what the latest release is — one `HEAD` request to `releases/latest`, bounded to five
seconds and fully asynchronous. When a newer release exists a toast says `hc-tui vX.Y.Z — ctrl+b u or click
↑vX.Y.Z`. The answer is cached in `$XDG_STATE_HOME/hc-tui/update-check.json` (`~/.local/state/hc-tui/` when XDG is
unset) for 24 hours, so at most one real request happens per day; a cached "newer" answer still shows the toast. Any
network or parse failure stays silent — no toast, no warning.

Opt out either way:

- `update.version_check = false` in your Herdr config, or
- `HC_TUI_NO_UPDATE_CHECK=1` in the environment.

## Uninstall

```sh
rm ~/.local/bin/hc-tui
```

hc-tui leaves little else behind:

- If you changed the theme, status indicators, sound or toast delivery in Settings, those are saved in your Herdr
  config. Other content in that file isn't touched.
- The SSH control socket lives in `$XDG_RUNTIME_DIR/hc-tui-ssh`, or `/tmp/hc-tui-ssh-<uid>`. It expires by itself, and
  you can delete the directory.
- The update-check cache is one JSON file in `$XDG_STATE_HOME/hc-tui/` (`~/.local/state/hc-tui/` when XDG is unset).
- The `hc-tui` layout document is stored on the core.

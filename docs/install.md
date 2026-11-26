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

How you upgrade depends on how backlot was installed. `backlot --upgrade` checks: on a package-managed install or a
dev build it refuses before anything runs and prints the command to use instead. An explicit `BACKLOT_INSTALL_DIR` is
taken as your own choice and runs the installer as asked.

### Install script and manual installs

```sh
backlot --upgrade
```

`--upgrade` runs the install script again for the latest release, with its progress on your terminal, and asks you to
restart backlot. Set `BACKLOT_VERSION=v<version>` first to move to a specific release, newer or older. Clients that are
already running keep the old version until you restart them. `--upgrade` installs into the directory of the running
binary (`BACKLOT_INSTALL_DIR` overrides it), so the command upgrades the binary you run.

Inside the client, `prefix+u` (or a click on the `↑v…` badge — or the compact `↑` it shrinks to when the sidebar
header is too narrow for the full label) opens the update card with the release notes of every
newer release. `↵ update` runs backlot's own install script locally, even when the client is attached to a remote
host, checks that `backlot --version` reports the new version, and then `↵ restart` re-executes the installed binary
in place. The card offers `↵ update` only when the install directory is writable; otherwise it shows the command to
run by hand. On a package-managed install the card never offers `↵ update` and names the package manager's own
command instead.

### mise

```sh
mise upgrade --bump github:followcat/backlot
```

`--bump` rewrites an exact version pin; a plain `mise upgrade` stays inside the version the config asks for and can
do nothing.

### Homebrew

```sh
brew upgrade backlot
```

### Nix

Update backlot through the Nix configuration that installed it; there is no single upgrade command.

### cargo

Reinstall with cargo the way backlot was installed: build from source as the README shows
(`cargo build --release -p hc-tui`, which produces the `backlot` binary) and replace the binary under
`$CARGO_HOME/bin` (`~/.cargo/bin` by default).

### Dev build

A binary under `target/` is a build artifact, not an install: rebuild it (`cargo build --release -p hc-tui`) and run
the new binary.

### Update check

On startup backlot asks GitHub what the latest release is — one `HEAD` request to `releases/latest`, bounded to five
seconds and fully asynchronous. When a newer release exists a toast says `backlot vX.Y.Z — ctrl+b u or click
↑vX.Y.Z`. The answer is cached in `$XDG_STATE_HOME/backlot/update-check.json` (`~/.local/state/backlot/` when XDG is
unset) for 24 hours, so at most one real request happens per day; a cached "newer" answer still shows the toast. Any
network or parse failure stays silent — no toast, no warning.

Opt out either way:

- `update.version_check = false` in your Herdr config, or
- `BACKLOT_NO_UPDATE_CHECK=1` in the environment.

## Uninstall

```sh
rm ~/.local/bin/backlot
```

backlot leaves little else behind:

- If you changed the status indicators, sound or toast delivery in Settings, those are saved in your Herdr
  config. Other content in that file isn't touched.
- backlot's own config is `~/.config/backlot/config.toml` (`$XDG_CONFIG_HOME/backlot/config.toml`): the theme you
  picked, the studio panel's settings and `[hollywood]`.
- The SSH control socket lives in `$XDG_RUNTIME_DIR/hc-tui-ssh`, or `/tmp/hc-tui-ssh-<uid>`. It expires by itself, and
  you can delete the directory.
- The update-check cache is one JSON file in `$XDG_STATE_HOME/backlot/` (`~/.local/state/backlot/` when XDG is unset).
- The `hc-tui` layout document is stored on the core.

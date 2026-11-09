# Install, upgrade, uninstall

hc-tui is a single binary. You also need the `hc` core: see
[herdr-core](https://github.com/herdrdev/herdr-core).

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

The script (`scripts/install.sh`):

1. picks the archive for your OS and architecture, and stops with a message if there isn't one;
2. finds the latest release, or uses the one in `HC_TUI_VERSION` (`v0.1.0` or `0.1.0`);
3. downloads `hc-tui-<version>-<target>.tar.gz` and `sha256sums.txt` over HTTPS;
4. checks the archive's checksum, and installs nothing if it doesn't match;
5. moves the binary into `${HC_TUI_INSTALL_DIR:-$HOME/.local/bin}`. It doesn't use sudo.

It tells you if the install directory isn't on your `PATH`. The checksum uses `sha256sum`, `shasum` or `openssl`,
whichever is available.

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

Run the install script again. Set `HC_TUI_VERSION=v<version>` to move to a specific release, newer or older. Clients
that are already running keep the old version until you restart them.

## Uninstall

```sh
rm ~/.local/bin/hc-tui
```

hc-tui leaves little else behind:

- If you changed the theme, status indicators, sound or toast delivery in Settings, those are saved in your Herdr
  config. Other content in that file isn't touched.
- The SSH control socket lives in `$XDG_RUNTIME_DIR/hc-tui-ssh`, or `/tmp/hc-tui-ssh-<uid>`. It expires by itself, and
  you can delete the directory.
- The `hc-tui` layout document is stored on the core.

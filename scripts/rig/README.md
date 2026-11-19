# Isolated visual test rig

`rig.sh` starts a throw-away backlot world for checking TUI changes by
hand or with screenshots: a local core, an optional second and third
core reached as `host-b`/`host-c` over `hc machine add --via`, neutral
git repositories with fake-runner studios, and optionally a private
Xvfb display with kitty running `backlot`.

Everything lives under one `ROOT` and every process is recorded in
`$ROOT/pids`. Teardown stops only those PIDs.

## Requirements

- `bash`, `git`, and a built `backlot`.
- A core binary: `herdr-core` (or an `hc` that provides `serve`).
- For the GUI: `Xvfb`, `kitty`, and `scrot` or ImageMagick `import`.
- Optional: Pillow for `--crop`, `python-xlib` for `xclick.py`.

Binaries are resolved from `PATH` or passed explicitly. Nothing is
hard-coded to a developer machine.

## Quick start

```sh
# Print the whole isolated plan first; spawns nothing.
scripts/rig/rig.sh plan --display :90

# Start two machines, a display and kitty.
scripts/rig/rig.sh up --display :90

# Work at the recorded display, then capture.
scripts/rig/rig.sh keys ctrl+b c
scripts/rig/rig.sh keys --text "hello"
scripts/rig/rig.sh shot /tmp/backlot.png --crop 1600x800+0+0

# Stop every recorded PID and remove ROOT.
scripts/rig/rig.sh down
```

Without a display, pass `--no-gui`; the cores, repositories, studios
and keys/status commands that do not need X still work.

`up` prints and stores the root, display and sessions. `status` is the
same summary later:

```sh
scripts/rig/rig.sh status --root /tmp/backlot-rig.XXXXXX
```

## Options

| Option | Meaning |
|---|---|
| `--root DIR` | Use `DIR` instead of `mktemp -d "${TMPDIR:-/tmp}/backlot-rig.XXXXXX"`. Must be empty. |
| `--display :N` | Use this X display; otherwise the first free display in `:90`..`:139` is picked. |
| `--size WxH` | Xvfb screen size, default `1600x1000`. |
| `--machines 1\|2\|3` | Local only, local + `host-b` (default), or also `host-c`. `host-c` has no `backlot` on its `PATH`, for "no backlot" checks. |
| `--backlot BIN` | backlot binary (env `BACKLOT_BIN`). |
| `--hc BIN` | hc CLI binary (env `HC_BIN`). |
| `--core BIN` | core binary (env `HC_CORE_BIN`); falls back to `herdr-core` or `hc`. |
| `--seed-latest vX.Y.Z` | Write an offline `update-check.json` so the version badge has a cached newer release. |
| `--no-gui` | Do not start Xvfb or kitty. |
| `--keep-root` | `down` keeps `ROOT` after stopping the processes. |
| `--dry-run` | `up --dry-run` is the same as `plan`. |

## Isolation and safety

Each machine gets its own `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`,
`XDG_RUNTIME_DIR`, `TMPDIR`, session name, socket and
`BACKLOT_STATE_DIR` under `ROOT`. Every core is started with `env -i`,
so these inherited names never reach a rig process:

```
HC_SCOPE HC_TERMINAL_ID HC_TERMINAL_SOCKET HERDR_ENV
WAYLAND_DISPLAY BACKLOT_STUDIO BACKLOT_ROLE BACKLOT_TASK
BACKLOT_STATE_DIR
```

The rig refuses `--root /` and `--root "$HOME"`, and `down` uses
`"${ROOT:?}"` before removing anything. It never connects to a named
session it did not create and never signals a process it did not
record.

## Layout

```
ROOT/
  home-{a,b,c}/  state-*/  config-*/  run-*/  bin-*/
  core-*.sock    kitty.sock   display   pids
  repos/acme-shop/  repos/beta-tools/
  cast.toml      via/host-b.sh  via/host-c.sh
  backlot-{a,b,c}/studios/
  log-*.log
```

`acme-shop` is created on the local core and `beta-tools` on `host-b`,
both with the shipped `fake` runner.

## Screenshots and clicks

`shot` uses `scrot` or `import` against the recorded display.
`--crop WxH+X+Y` crops in place through Pillow.

`xclick.py` sends raw X input through XTest. It never assumes `:0`:

```sh
scripts/rig/xclick.py --display :90 click 320 240
scripts/rig/xclick.py --display :90 click 320 240 --button 3
scripts/rig/xclick.py --display :90 drag 100 100 700 500
scripts/rig/xclick.py --display :90 --dry-run drag 1 2 3 4
```

`--display` or `RIG_DISPLAY` is required; `--dry-run` works without
`python-xlib` and prints exactly what would be sent.

## Tests

```sh
python3 -m unittest discover -s scripts/rig -p 'test_*.py'
```

The suite checks shell syntax, the complete `plan` output, unsafe-root
and bad-seed refusals, that `down` kills only the recorded PID and leaves
no `TMPDIR` files, and `xclick.py`'s dry-run and display requirement.

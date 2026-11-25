# Single backlot server (`backlot server`)

`backlot server` runs one machine-wide backlot daemon under `~/.local/state/backlot/server/`
instead of one per-core `backlot-stage` daemon per `hc` session. It runs the same supervisor loop
under a dedicated `hc` session (default `--session backlot`) and holds an exclusive OS lock so no
second local `backlot-server` or legacy `backlot-stage` runs alongside it.

## State directory and lock

- Server state lives at `$XDG_STATE_HOME/backlot/server/` (default `~/.local/state/backlot/server/`),
  or `BACKLOT_STATE_DIR` when explicitly set:
  `studios/`, `hollywood/`, `archive/`, `config.toml`, `live.json`, `server.lock`, and `server-lease.json`.
- `backlot server run` acquires `server.lock` with `flock(2)` and maintains a liveness lease
  (`server-lease.json`, renewed every 5 s while active). On a crash, the OS releases the `flock`,
  so a stale `server-lease.json` never blocks restart.
- `backlot studio new` is local-only in Phase 1; passing `--remote` refuses:
  - `"remote studio placement is Phase 2; create studios on the server host"`

## Running and controlling the server

Run directly in the foreground (used interactively or by systemd — does not require `BACKLOT_STAGE=1`):

```sh
backlot server run --session backlot
```

Or manage it inside an `hc` session (mirroring `backlot stage start|stop`, defaulting to `--session backlot`):

```sh
backlot server start --session backlot
backlot server stop --session backlot
```

While `backlot-server` is running on a machine:

- A second `backlot server run` (in any `--session`) refuses with one line naming the running
  server and session:
  - `"backlot-server is already running (--session backlot, pid <pid>)"`
- `backlot stage start` or `backlot stage run` in any local session refuses with one line naming
  `backlot-server` and its session:
  - `"backlot-server is already running (--session backlot); cannot start backlot-stage"`

## Adopting existing `<core_id>` state

`backlot server adopt` migrates an existing per-core `~/.local/state/backlot/<core_id>/` layout
to `server/` without rewriting live git worktrees:

```sh
backlot server adopt --dry-run
backlot server adopt --stop-stages --core core-id
```

1. **Running-stage check:** if a legacy `backlot-stage` terminal is running, `adopt` refuses
   unless `--stop-stages` is passed (which stops those terminals cleanly first).
2. **Primary promotion:** moves `{studios,hollywood,archive,config.toml}` (plus `agent.env` and
   `processed-reqs.jsonl` if present) from the selected `<core_id>` into `server/`, and creates
   relative `../server/...` symlinks back in `<core_id>/` so live git worktrees and pinned
   `BACKLOT_STATE_DIR` paths keep working untouched.
3. **Secondary/empty cores:** archives any other `<core_id>` directories under
   `server/adopted-<stamp>/`.
4. **Tombstones:** writes `server-pointer.json` in every retired `<core_id>/`. Future `backlot stage start`
   calls targeting that core refuse with:
   - `"managed by backlot-server (--session backlot)"`

`--dry-run` prints the planned stops, moves, symlinks, archives, and tombstones and leaves the
filesystem and running terminals untouched.

## Systemd user unit

```sh
backlot server service --print --session backlot
backlot server service --write --session backlot
```

`--write` installs `${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/backlot-server.service`
(or `backlot-server-<s>.service` when `--session` differs from `backlot`) configured with
`Restart=always`, `RestartSec=2`, and `ExecStart=<backlot> server run --session <s>`, and prints
the `systemctl --user daemon-reload` and `systemctl --user enable --now ...` commands without
running `systemctl`.

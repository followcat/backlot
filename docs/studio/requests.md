# Request, Result, and Catalog Documents (`plugin.backlot.*` & `backlot-cat`)

Clients (`backlot` TUI/CLI remotes and `hc` document clients) drive person actions over the stage's `hc` session using compare-and-set (`doc.get` / `doc.set`) on three fixed documents matching `hc`'s name rule (`^[a-z0-9_][a-z0-9_.-]{0,31}$`):

| Document | Writer | Purpose |
| --- | --- | --- |
| `plugin.backlot.req` | Clients + Stage drain | Bounded person-action request queue (`max 32`) |
| `plugin.backlot.res` | Stage only | Bounded request ack ring (`last 64`, `<= 64 KiB` result) |
| `backlot-cat` | Stage only | Catalog of runners, `[hollywood]`, `default_cast`, archive |

Every struct uses additive-only evolution within schema version 1; unknown fields in documents are ignored on read, and unknown or malformed queue items yield per-entry error acks without wedging sibling requests.

---

## Trust Boundary

**Document writes are anonymous and are not a privilege boundary.** `herdr-core` `doc.set` carries no caller or terminal identity; any local process holding the user's `hc` socket can write `plugin.backlot.req`, just as it can edit ledgers on disk under the user's UID.

Within that user-equivalent boundary, the stage enforces hard safety invariants before touching ledgers:

1. **No prompt or dialog answering (`approve` refusal):** `approve`, `permission`, `dialog`, `answer`, `prompt`, `prompt-answer`, and `role-approval` have no request verb and always fail with an error ack pointing back to the agent's own conversation or terminal. Interactive permission prompts and trust dialogs are only answered directly by the human in the agent screen.
2. **No outward or shell actions:** `exec`, `shell`, `run`, `terminal`, `terminal-send`, and `outward` are refused.
3. **Person verbs only:** only the explicit typed person verbs listed below are accepted; role-only actions and unpermitted subcommands are refused.
4. **Non-prompt attentions only:** `attention-clear` refuses interactive prompt/dialog attentions (`permission prompt`, `question`, `folder-trust`, `folder trust refused`, `* prompt`, `*dialog*`).
5. **Confirm tokens on destructive verbs:** `studio-delete` and `studio-prune` always, and `studio-restore` when its dry run renames the copy. A dry run returns `{ report, confirm_token }`, where `confirm_token` is `sha256` of the report's canonical (compact) JSON; the commit must echo that exact token and is re-checked against a fresh dry run.
6. **Audit trail:** every ledger event recorded through `plugin.backlot.req` carries `by: { role: "user", via: "doc" }` (or `{ role: "stage", via: "doc" }` for stage-recorded `message.seen` under `answers`).

---

## Wire Schemas

### `plugin.backlot.req` (`schema: "backlot-req/1"`)

```json
{
  "schema": "backlot-req/1",
  "queue": [
    {
      "req_id": "req-1732500000000-1234-1",
      "ts_ms": 1732500000000,
      "action": { "kind": "tell", "studio": "acme-shop", "role": "producer", "text": "status check" },
      "confirm_token": null
    }
  ]
}
```

- **Queue limit (`32`):** clients fail cleanly with `"server busy"` when `queue.len() >= 32` (retrying up to 3x on `conflict`). Any overflow entries past index 32 written directly also receive a `"server busy"` error ack without executing.
- **Drain semantics:** the stage removes only handled `req_id`s with `doc.set` CAS; on `conflict`, it re-reads `plugin.backlot.req` and removes only handled ids, preserving concurrent client appends.
- **Wake & poll cadence:** the stage wakes immediately on local `doc` notifications for `plugin.backlot.req` (ignoring remote-machine `machine != null` events), also polls once on startup, and polls every 60 s in case an event was missed.

### `plugin.backlot.res` (`schema: "backlot-res/1"`)

```json
{
  "schema": "backlot-res/1",
  "acks": [
    {
      "req_id": "req-1732500000000-1234-1",
      "ts_ms": 1732500000012,
      "ok": true,
      "result": { "studio": "acme-shop", "role": "producer", "seq": 9, "file": "messages/9-doc-tell.txt" }
    }
  ]
}
```

- **Caps:** keeps at most the newest **64** acks; each `result` payload is capped at **64 KiB** (`65,536` bytes — oversized payloads degrade to an error ack); oldest acks are trimmed if needed so the encoded document stays well below `herdr-core`'s 1 MiB limit.
- After draining handled entries, the stage immediately republishes `backlot` (and `backlot-cat` if changed).

### `backlot-cat` (`schema: "backlot-catalog/1"`)

```json
{
  "schema": "backlot-catalog/1",
  "server_id": "core-1",
  "ts_ms": 1732500000000,
  "runners": [
    { "runner": "claude", "status": "found", "version": "1.x", "path": "/usr/local/bin/claude", "needs_path": false, "models": ["..."], "efforts": ["low", "medium", "high"] }
  ],
  "hollywood_cfg": { "configured": true, "runner": "claude", "model": "sonnet", "effort": "high" },
  "default_cast": { "producer": {}, "director": {}, "writer": {}, "candidates": [], "limits": {} },
  "archived": [
    { "name": "acme-shop-20261125T000000Z", "stamp": "20261125T000000Z", "studio": "acme-shop", "repo": "/home/user/acme-shop", "requirement": "v1", "last_ms": 1732490000000 }
  ],
  "home": "/home/user"
}
```

- Never publishes secrets (`[hollywood].key` is omitted).
- Published at stage start and republished whenever any catalog field (`runners`, `hollywood_cfg`, `default_cast`, `archived`, `server_id`, `home`) changes.

### `backlot` summary projection (`backlot/1`)

The stage publishes the live studio summary under document `backlot` (`schema: "backlot/1"`). Two additive, backward-compatible extensions on each `StudioSummary` entry support forward compatibility and the opening design discussion (missing fields default safely on older readers):

- **`needs_upgrade` (`bool`, default `false`) & `unknown_kinds` (`string[]`):** present only when a studio's ledger contains `EventKind::Unknown` records at `v <= LEDGER_VERSION`. Clients display `"upgrade backlot"` and keep that studio read-only.
- **`design` (`DesignSummary?`, omitted when `null`):** present when `design_gate == "required"` and the current `requirement_version` has opened or signed design state (`phase != "idle"`). Fields:
  - `version` (`u32`), `round` (`u32`), `phase` (`"idle" | "suggest_skip" | "waiting_proposal" | "waiting_buildability" | "waiting_summary" | "waiting_user" | "signed"`)
  - `waiting_user` (`bool`, `true` in `"waiting_user"` or `"suggest_skip"` — routes Call Sheet `DECISION` items to the warm `?N` asks bucket rather than blocking `!B`)
  - `suggest_skip` (`bool`), `skip_reason` (`string?`), `doc_file` (`"design/v<N>.md"`), `summary` (`string?`), `questions` (up to `3` `{ id, prompt, recommendation, section_ref? }`), `waiting_since_ms` (`u64?`, set only while `waiting_user == true`)
  - `summary`, `skip_reason`, `doc_file`, and question strings are capped at 96 characters (`CAP = 96`); the complete Markdown lives in `doc_file` (`design/v<N>.md`, or `design/v<N>-final.md` after `approved` sign-off). Once `phase == "signed"` and all tasks for `version` have a cast, `design` shrinks to header fields (`summary: null`, `questions: []`).

---

## Supported Actions & Results (`B1a`)

| `action.kind` | Fields | Typed `result` fields |
| --- | --- | --- |
| `tell` | `studio`, `role`, `text` | `studio`, `role`, `seq`, `file` |
| `tell-hollywood` | `text`, `answers?` (`u64[]`) | `seq`, `to`, `marked_seen?` |
| `card-apply` | `id` (`"cN"` or `N`) | `id`, `seq`, `outcome_seq`, `stale?`, `summary` |
| `card-cancel` | `id`, `reason?` | `id`, `seq`, `outcome_seq`, `reason?` |
| `card-show` | `id` | `id`, `seq`, `state`, `action_label`, `action`, `base`, `why`, `by`, `ts_ms`, `detail?`, `stale?`, `identity_change?` (`old`, `new`, `old_label`, `new_label`), `studio_new?` (`title`, `repo`, `no_git`, `cast_path`, `brief_path`, `cast_text?`, `brief_text?`, `draft_error?`) |
| `attention-clear` | `studio`, `target` (or `seq`) | `studio`, `target`, `seq`, `cleared_role`, `cleared_reason` |
| `recast` | `studio`, `person`, `runner`, `model`, `effort?`, `permission_profile?`, `reason?` | `studio`, `person`, `seq`, `old`, `new`, `reason?` |
| `restart` | `studio?`, `person` (supports `"hollywood"`), `fresh?`, `reason?` | `studio?`, `person`, `seq`, `fresh?`, `reason?`, `is_hollywood?` |
| `close` | `studio`, `person` | `studio`, `person`, `seq` |
| `reopen` | `studio`, `person` | `studio`, `person`, `seq` |
| `task-priority` | `studio`, `task`, `level` (`low` \| `normal` \| `high`) | `studio`, `task`, `seq`, `level` |
| `task-pause` | `studio`, `task`, `reason?` | `studio`, `task`, `seq`, `state` (`paused`), `reason?` |
| `task-resume` | `studio`, `task`, `reason?` | `studio`, `task`, `seq`, `state` (`resumed`), `reason?` |
| `task-abandon` | `studio`, `task`, `reason?` | `studio`, `task`, `seq`, `state` (`abandoned`), `reason?` |
| `hollywood-setup` | `runner`, `model`, `effort?`, `key?`, `default_cast?`, `force_offline?` | `runner`, `model`, `effort?`, `key?`, `default_cast?`, `action` (`configured` \| `updated`), `restarted` |
| `hollywood-switch` | `runner`, `model`, `effort?`, `timeout_s?`, `force?` | `action` (`queued`), `handoff?` (`HandoffSummary`) |
| `hollywood-switch-cancel` | — | `action` (`cancel-requested`), `handoff?` (`HandoffSummary`) |
| `hollywood-switch-status` | — | `active` (`bool`), `handoff?` (`HandoffSummary`) |
| `design-sign` | `studio`, `skip?`, `reason?`, `note?` | `studio`, `version`, `round`, `mode` (`approved` \| `skipped`), `doc_file`, `seq` |
| `design-start-round` | `studio` | `studio`, `version`, `round`, `doc_file`, `seq` |

During `waiting_user`, a `tell` targeting `producer` also appends a `design.entry` (`slot: "user_input"`, `source: "tell"`) and updates `design/v<N>.md`. `design-sign` and `design-start-round` execute with `by: { role: "user", via: "doc" }`, satisfying the user-only sign-off gate.

### Studio lifecycle (`B1b`)

| `action.kind` | Fields | Typed `result` fields |
| --- | --- | --- |
| `studio-new` | `id`, `title`, `repo` (a hub path), `cast` (TOML text), `mode?`, `init?`, `no_git?`, `ask?`, `dry_run?` | `studio`, `title`, `repo`, `dry_run?`, `seq?`, `ask_seq?`, `first_message?` |
| `studio-finish` | `studio`, `reason?`, `force?` | `studio`, `seq`, `reason` |
| `studio-prune` | `studio?`, `all_finished?`, `dry_run?`, `force?` | `PruneReport { studios: [{ studio, finished, dry_run, steps }] }`, or `{ report, confirm_token }` on a dry run |
| `studio-delete` | `studio?`, `dry_run?`, `force?` | `DeleteReport`, or `{ report, confirm_token }` on a dry run |
| `studio-restore` | `archived`, `name?`, `dry_run?` | `RestoreReport`, or `{ report, confirm_token }` on a dry run |
| `repos` | `from?`, `all?` | `{ cwd, entries: [{ name, path, kind, in_use_studio, branch, members? }] }` |
| `runners` | — | `RunnerCatalogEntry[]` (`runner`, `status`, `version`, `path`, `needs_path`, `models`, `efforts`) |
| `version` | — | the hub binary's version line (a JSON string) |

The cast and the first message are content, never client paths; the hub writes them to its own temporary files, canonicalizes `repo` against its own filesystem, and runs the same code the local command runs. A dry run's `confirm_token` is required on the matching commit; `studio-restore` requires it only when the dry run reports `renamed: true` (replacing an existing studio is refused outright). Card approvals `card-apply` and `card-cancel` travel over the same queue.

---

## Expiry & Deduplication

- **Expiry window:** requests with `ts_ms` more than **60 s** older than the stage clock are refused with `"request expired"`. Up to **60 s** of future clock skew is accepted; further-ahead timestamps are refused.
- **Restart-safe deduplication:** handled `(req_id, ts_ms, ack)` records append to `<state_dir>/processed-reqs.jsonl` before queue drain. Replayed `req_id`s never execute a verb twice across stage restarts, and entries older than **24 h** are pruned automatically.

---

## Client Routing & Error Lines (`B2`)

- **Server resolution:** both `backlot` TUI and CLI resolve `--server <host>[/<session>]` (default session `backlot`), then `BACKLOT_SERVER`, then `[client] server` / `session` in `~/.config/backlot/config.toml`.
- **Off-hub vs on-hub:** off-hub clients connect over `ssh <host> hc connect --session <session>`, read `backlot` + `backlot-cat`, and submit the B1a person verbs, card approvals and the B1b studio lifecycle actions via `plugin.backlot.req` -> `plugin.backlot.res` (5 s ack timeout). When the resolved host matches the local hub, the CLI keeps writing the local ledger directly (still works while `backlot-server` restarts) while TUI connects locally to the named session.
- **Client errors & stopped server:** `"server busy"` (full queue or 3 CAS conflicts), `"request expired"`, `"timed out waiting for ack of '<req_id>' (no reply from server)"`, `"confirm this from a dry run first"` / `"the studio changed since you reviewed it; run it again"` on an unreviewed or stale destructive commit, and `"not available through the server yet"` for the verbs the queue still does not carry (delivery, review, correction, approval, `req revise`, `plan publish`). When `backlot-cat` is present with no live `backlot-server` or `backlot-stage` terminal, the TUI displays the start command (`systemctl --user start backlot-server.service` or `backlot server start --session backlot`) without spawning anything itself.
- **What guards the queue:** remote clients approve over the server queue; role terminals cannot use `hc run|exec|send|keys|connect`, interpreter code (`python -c`, `node -e`, a heredoc) that writes `plugin.backlot.req` or `doc.set` or reads the core socket, or detach tricks such as `systemd-run` and `tmux new -d`, because roles run as the person's own user and this is a seatbelt against an overeager agent, not isolation.

# Request, Result, and Catalog Documents (`plugin.backlot.*` & `backlot-cat`)

Clients (`backlot` TUI/CLI remotes and `wherdr`) drive person actions over the stage's `hc` session using compare-and-set (`doc.get` / `doc.set`) on three fixed documents matching `hc`'s name rule (`^[a-z0-9_][a-z0-9_.-]{0,31}$`):

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
5. **Audit trail:** every ledger event recorded through `plugin.backlot.req` carries `by: { role: "user", via: "doc" }` (or `{ role: "stage", via: "doc" }` for stage-recorded `message.seen` under `answers`).

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
| `hollywood-setup` | `runner`, `model`, `effort?`, `key?`, `default_cast?` | `runner`, `model`, `effort?`, `key?`, `default_cast?`, `action` (`configured` \| `updated`), `restarted` |

*(Studio lifecycle verbs `new`, `finish`, `prune`, `delete`, `restore`, and `repos` are B1b.)*

---

## Expiry & Deduplication

- **Expiry window:** requests with `ts_ms` more than **60 s** older than the stage clock are refused with `"request expired"`. Up to **60 s** of future clock skew is accepted; further-ahead timestamps are refused.
- **Restart-safe deduplication:** handled `(req_id, ts_ms, ack)` records append to `<state_dir>/processed-reqs.jsonl` before queue drain. Replayed `req_id`s never execute a verb twice across stage restarts, and entries older than **24 h** are pruned automatically.

---

## Client Routing & Error Lines (`B2`)

- **Server resolution:** both `backlot` TUI and CLI resolve `--server <host>[/<session>]` (default session `backlot`), then `BACKLOT_SERVER`, then `[client] server` / `session` in `~/.config/backlot/config.toml`.
- **Off-hub vs on-hub:** off-hub clients connect over `ssh <host> hc connect --session <session>`, read `backlot` + `backlot-cat`, and submit `B1a` verbs via `plugin.backlot.req` -> `plugin.backlot.res` (5 s ack timeout). When the resolved host matches the local hub, the CLI keeps writing the local ledger directly (still works while `backlot-server` restarts) while TUI connects locally to the named session.
- **Client errors & stopped server:** `"server busy"` (full queue or 3 CAS conflicts), `"request expired"`, `"timed out waiting for ack of '<req_id>' (no reply from server)"`, and `"not available through the server yet"` (uncovered verbs such as `studio new`/`finish`/`prune`/`delete`/`restore`). When `backlot-cat` is present with no live `backlot-server` or `backlot-stage` terminal, the TUI displays the start command (`systemctl --user start backlot-server.service` or `backlot server start --session backlot`) without spawning anything itself.

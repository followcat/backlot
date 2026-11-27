# Studio metrics and digest

`backlot metrics` and `backlot digest` read studio ledgers and Hollywood's ledger without spawning
any agent, mutating state, or calling models. Unrecognized `unknown` events are skipped and counted
in `unknown_events` / `unknown_events_skipped` so newer ledgers stay readable.

## Commands

```sh
backlot metrics --all
backlot metrics --studio acme-shop --since 7d --json
backlot digest --studio acme-shop --since 24h
backlot digest --all --format json
```

- `--studio <id>` and `--all` are mutually exclusive; omitting both covers every active studio.
- `--since <dur>` accepts `ms`, `s`, `m`, `h`, `d`, `w` and compound spans (`1d12h`).
  - `backlot metrics` defaults to full history; `--since <dur>` keeps events with `ts_ms >= now_ms - dur`.
  - `backlot digest` defaults to `--since 24h` and `--format md`.
- `backlot metrics` renders tables `<= 100` columns wide; `--json` prints the structured document.
- `backlot digest --format md` prints per-studio activity, pending human items, rate limits, and
  the requirement coverage matrix. `--format json` emits the matching JSON document.

## Metric definitions

All ratios return `null` in JSON (and `-` in tables) when their denominator or sample count is 0.
`all` pools raw samples across selected studios rather than averaging per-studio medians or rates.

- **Quality**
  - `delivered_tasks`: tasks with at least one `task.delivered` (whose first delivery falls in `--since`).
  - `first_delivery_accepted`: subset whose first delivery received `task.reviewed` with `verdict = accepted`.
  - `first_delivery_pass_rate`: `first_delivery_accepted / delivered_tasks`.
  - `deliveries`: `task.delivered` events in window.
  - `changes_requested_reviews`: `task.reviewed` events in window with `verdict = changes_requested`.
  - `changes_requested_rate`: `changes_requested_reviews / deliveries`.
  - `early_tasks`: `task.created` events in window recorded with `seq` earlier than the first `dispatch` message (`purpose = "dispatch"`) of that `requirement_version` (when a dispatch exists for that version).
- **Checks and coverage**
  - `configured`: `true` when `studio.created` declared non-empty `checks`. In `all`, `studios_with_checks / studios_total` is reported as `checks_coverage_rate`.
  - `task_checked`, `task_checked_passed`, `task_checked_failed`: `task.checked` runs in window (`passed = exit_code == 0 && !timed_out`).
- **Findings**
  - `task_finding_events`, `findings`: `task.finding` event count and total defect items in window.
- **Continuity**
  - `director_actor_tells`: `message.queued` in window with `from = "director"`, recipient an actor (`actor-<task>-c<n>`), `purpose = null`, and `source_seq = null`.
  - `tells_per_task`: `director_actor_tells / created_tasks`.
- **Attention and guards**
  - `by_class` (`needs_you`, `stuck`, `info`): `attention.raised` events in window grouped via `AttentionClass::of(reason, needs_user)`, reporting `count`, currently `open` count, and total `open_ms`. Open raises bound at `min(now_ms, finished_ts_ms)` if the studio is finished, or `now_ms`.
  - `needs_user_count`: raises in window with `needs_user = true`.
  - `permission_prompts_not_needs_user`: `permission prompt` raises in window with `needs_user = false` (`count` and `open_ms`).
  - `rate_limited`: `reason = "rate limited"` raises in window clustered per `(role, reset_window)` where matching `reset_at_ms` within 60s belong to one window; reports `total`, `windows`, `max_per_window`, and `avg_per_window`.
  - `approvals`: `role.approval` decisions (`total`, `allow`, `deny`).
  - `refused`: `role.refused` events (`total`, `without_probes`, `probes_dropped_by_209`, and per-rule counts) separating benign read-only git probes (`git --version`, `git rev-parse --show-toplevel`, including `-C <path>` prefixes).
- **Hollywood**
  - `notices_sent`, `notices_dropped`, `drop_rate`: Stage-to-Hollywood messages (`from = "stage"`, `to = "hollywood"`) in Hollywood's ledger filtered to selected studios (plus any unscoped stage notices when `--all`).
- **Timing**
  - `opening_ms` (`opening_p50_ms` across studios): first `req.version` to first `dispatch` message.
  - `review_latency_p50_ms` / `p90_ms`: linear-interpolated percentiles over `reviewed.ts_ms - delivered.ts_ms`.
  - `task_wall_clock_p50_ms`: `closed_ts_ms - first_cast.ts_ms` on `accepted` tasks.
  - `pause_clusters` / `paused_tasks_in_clusters`: maximal runs of `>= 2` consecutive `task.state=paused` events (ignoring `unknown` events).

## Digest and coverage matrix

`backlot digest` summarizes each studio across `[now_ms - since, now_ms]`:

- **Finished**, **Delivered**, **Sent back (`changes_requested`)**, **Blocked**, **Waiting on person** (open `NeedsYou` attentions, unanswered `tell user` asks, waiting design gate), and **Rate limits** (open rate-limit attentions plus window cluster counts).
- **Coverage matrix**: maps `design section -> task -> commits -> task.checked -> verdict`.
  - Citations matching `design v<N> §<id>` in a task's `acceptance` file inside the studio directory produce one row per `(section, task)`; tasks without citations produce one unsectioned row.
  - Studios without configured checks display `checks: none` cleanly and do not error.

## JSON schema (`schema_version = 1`)

- `backlot metrics --json`: `{ schema_version, since_ms, now_ms, unknown_events_skipped, studios: [StudioMetrics...], all: AggregateMetrics }`.
- `backlot digest --format json`: `{ schema_version, since_ms, now_ms, unknown_events_skipped, studios: [StudioDigest...] }`.

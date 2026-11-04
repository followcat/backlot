# Sidebar semantics (M2.2)

What the sidebar model computes from a `watch::View`, and what the words mean.
Everything here is *reported* state: hc-tui never infers status from terminal
output, argv or foreground process names.

## Status authority

A terminal's status comes only from the record fields core publishes:

- `status` — the program's root OSC 7501 record;
- `records` — named 7501 records (a program reporting on several things);
- `reported` — whether it ever sent a 7501 report.

The *effective word* is the most urgent over the root record and every named
record. Ties prefer the root record, then records by name. The reported
word always leads the badge — a winning named record trails as `word·name`
and a patched core's extension fields (e.g. `source`) as `·source` — so a
clipped badge keeps the primary word ahead of optional detail; an unusually
long word may itself be ellipsized.
Extensions are carried verbatim, never interpreted.

Known words: `blocked`, `error`, `working`, `done`, `idle`. Unknown or newer
words are kept verbatim — never relabeled working or blocked.

## Lifecycle

Independent of the reported word, the program's lifecycle is read from
`state`/`pid`/`reason` exactly as core defines it:

- `state = "finished"` → **finished** (with exit code/signal/reason);
- `live` without `pid` and `reason` present and not `spawn_pending` → **lost**
  (lost-live; the holder died or the machine went away mid-run);
- `live` without `pid` and no reason (or `spawn_pending`) → **pending**
  (still starting);
- `live` with a pid → **live** — the reported word applies.

A machine's state (`connected`, `connecting`, `needs_login`, `unreachable`,
or newer words) is shown on its heading; anything but `connected` renders its
terminals dimmed — machine offline, terminal lost and program finished stay
three distinct things. The local machine takes its state from the watcher's
subscription link; its last inventory stays listed while offline. Remote
freshness has two sources: terminals flow through the subscription, while
the machine's own state word comes from `machine.list` over the rpc link.
With the subscription dead (or the watcher offline) every remote row is a
cached view — heading marked `·stale`, dimmed, counted offline. With only
rpc dead, the state word gets the `·stale` hint but terminals keep
streaming: not dimmed, not counted offline.

A lifecycle `state` this build does not know (`live`/`finished` are the
documented two) is kept verbatim, never folded into finished.

## Agents

An **agent** is a terminal that carries status: `reported`, a root `status`
record, or any named `records`. Plain shells never appear in the agents view
and never enter counters.

Workers are terminals with a `parent` address. Parent addresses are matched
verbatim — core already localizes them, so a remote worker can legitimately
name a bare local `tN`; the machine prefix is never re-applied. A worker shows
`worker of <parent>` whether or not the parent is still known; unknown
cross-core parents stay visible. Every terminal counts at most once, worker
or not.

## Counters

Counted over *live* agents only — a finished, lost or still-starting
program's last report is not a live agent:

- **your turn** — live agents whose effective word is `blocked`;
- **working** — live agents whose effective word is `working`;
- **ready** — live agents whose effective word is `done`.

## Urgency

For space aggregation and agents-view ordering (most urgent first):

`blocked` > `error` > lost > `working` > unknown words > `done` > `idle`
> pending/unreported live > `finished`.

Spaces use a stable order within each machine. Agents sort by urgency, then
local-first machine order and terminal number — deterministic everywhere.

## Spaces

A terminal's space is its `herdr.space` label when present, else its machine
plus `cwd`. The fallback space displays the directory's basename and keys on
the full path, so two directories sharing a basename stay distinct. Terminals
sort by terminal number inside a space. Machines list local first, then by
name; a terminal whose address names a machine `machine.list` does not
report still gets its own group rather than being dropped.

Under its own machine heading a terminal shows only its local id (`t7`) —
the `alias/` prefix is redundant there. In the agents list the qualified
address (`alias/t7`) is the only machine info and stays.

## Selection and layout

Selection tracks a stable row identity — a terminal address, a
`(machine, space)` pair, or a machine name — not a list index. After a
rebuild the same identity keeps its new position; a removed identity falls
back to whatever row now sits at its old position (clamped), which is
deterministic. Lists scroll so the selection stays visible.

At 80 columns and above the sidebar renders expanded beside a reserved pane
area; below that it collapses to a compact status line carrying the counters.
The user toggle records a preference that still applies at narrow widths.
Every API string is sanitized before rendering (C0/C1/DEL/ESC removed,
whitespace controls flattened to spaces) and truncated by display width on
grapheme boundaries — no sequence can reach the host terminal.

When a row cannot fit, identity and badge share a bounded budget. The
left text keeps up to 12 cells including indentation; a qualified address
that cannot fit there is head-elided. Thus `…/t7` and `…/t8` under one
long machine alias stay distinguishable while the primary state word also
remains visible at the normal sidebar widths.

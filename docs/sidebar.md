# Sidebar data and presentation

The sidebar follows herdr **0.9.5-followcat at `d00953e0`**. Its renderer,
configuration, glyphs, colors, ordering, truncation, selection and mouse
behavior come from the reference source. See [parity](parity.md) for scope,
data mappings and validation. M2 implementation and final conformance checks
are still in progress.

## Reported status

Core is the authority for status. hc-tui does not infer it from screen text,
program arguments or foreground process names. It reads:

- `status`, the root OSC 7501 record;
- `records`, named OSC 7501 records;
- `reported`, whether a terminal has reported status.

The effective record is the most urgent root or named record. Record
authority uses `blocked > error > working > unknown > done > idle`.
Ties prefer the root, then named records in name order. This chooses the
record; it does not define the displayed sidebar order.

The winning record supplies the agent's application and reported title.
Unknown words and extension fields remain in the protocol inventory. The
renderer consumes herdr's status vocabulary, with the exact mapping recorded
in [parity](parity.md#status-vocabulary). Record names, `source` extensions,
progress and blocked-kind fields do not add hc-tui-specific badges.

## Program lifecycle

Lifecycle is separate from a program's last report:

| Core facts | Lifecycle |
|---|---|
| `state = "finished"` | Finished; retain exit code, signal and reason |
| `state = "live"`, no pid, reason other than `spawn_pending` | Lost |
| `state = "live"`, no pid, no reason or `spawn_pending` | Pending |
| `state = "live"`, pid present | Live |
| Another `state` word | Preserve the unfamiliar lifecycle |

A finished, lost or pending program's old report does not make it a live
agent. Its terminal stays in the inventory. A lost connection retains the
last inventory and pane frame; it does not change program lifecycle.

An agent is a live terminal with `reported`, a root record or named records.
Shells without reports do not become agents. Each agent appears once.
Workers keep their own terminal identity and endpoint. Core parent addresses
are already localized and must be matched verbatim; a bare parent is not
automatically prefixed with the worker's machine. The reference sidebar has
no separate parent-note field, so workers use its ordinary agent rows.

## Spaces, tabs and titles

M2 groups terminals by `herdr.space`, falling back to endpoint plus full cwd.
Only the cwd basename is displayed. Identical basenames with different paths
remain different spaces. Within a space, `herdr.tab` identifies a tab; an
unlabelled tab is distinct from an explicitly labelled `default` tab and
displays its numeric position. Creation order and terminal-number tie breaks
provide stable ordering. M3 adds the herdr-app document reader and shared
layout state.

Identity uses the raw endpoint, space and tab values. Sanitizing or clipping
display text must not merge identities or change focus. Every API string
that reaches the display is control-sanitized and fitted using the ported
text helpers.

Titles remain distinct data:

- `labels["herdr.title"]`: the manual pane label;
- the winning status record's `title`: the reported agent title;
- terminal `title`: the program's OSC title;
- terminal `name`: the core terminal's optional name.

These feed separate reference fields. The ported renderer decides title
precedence; available titles must not be replaced by invented terminal-id
labels.

## Attention, seen state and order

Herdr's display priority is `Blocked > Done > Working > Idle > Unknown`.
Workspace and tab aggregates use that priority. The default agent sort is
herdr's configured default (space order); an explicit priority sort uses
status-change recency and deterministic creation ties.

Completion acknowledgement follows the reference transitions: an initial
idle agent is seen, background work that finishes becomes done, and focusing
its active tab acknowledges the completion without requiring a new report.
A new turn or program incarnation must not inherit a stale acknowledgement.
M2 keeps this presentation state locally and does not write core labels.
The missing shared, persistent acknowledgement is a documented data gap.

Per-terminal `turn` values are not a global chronology. Use available status
timestamps and observed transitions for recency; unknown pre-connection
history is documented in [parity](parity.md#core-facts-and-replacements).

## Machines, selection and geometry

Core machine and connection states map to herdr's endpoint states. The
reference's stale presentation applies to retained endpoint snapshots.
Subscription loss and RPC-only loss have different data freshness; neither
may discard cached terminals or invent program-status changes. Machines
without terminals and terminals with an unlisted machine prefix remain in
the projection; the reference controls which headings are visible.

Selection follows stable identities across inventory reordering. Navigation
highlight and committed focus are distinct, as in herdr. Lists reveal the
selected row using the reference scrolling rules.

Sidebar widths, collapse behavior, mobile header, tab bar, pane gutters and
borders use the reference configuration and geometry. Narrow terminals do
not introduce a separate hc-tui status-line format. Divider dragging updates
both drawing and the size allocated to the attached program.

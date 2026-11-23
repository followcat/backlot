# Studio panel

The right-hand studio column (S4b, reworked by U1 and U2). Everything below is client-local state — per
client, in memory only. Under `backlot --plain` the client is cell-identical to herdr.

The panel and the sidebar's cast are viewers: what they show comes from the `backlot` document and
the terminal inventory; what they change goes through `backlot` commands and the existing focus
path.

## Studio mode

Since U2, `backlot` is **always** in studio mode — the mode is `not --plain`, it does not depend
on the document. A missing, malformed or foreign-schema document, a good one with zero studios,
or a stopped stage never flips it: the cast then shows its no-studio view (the header alone,
then `others`) and the opening page covers the panes. The only way out is `backlot --plain`:
there the client is exactly herdr — the agents list, herdr's sidebar width, herdr's key
bindings.

The **current studio** is the studio whose person owns the focused terminal. When the focus
belongs to no studio the current studio stays what it was; at the start it is the first studio in
document order. The client remembers, per studio, the person it last focused there.

## Toggle

`prefix+y` cycles: closed → expanded and focused → (unfocused) focused → closed. The dock opens
expanded by default; `--plain` starts it closed for the parity harness's frame. `y` is unbound
in herdr's defaults and the parity scripts never send it. A configured herdr binding on the same
tail still wins while the panel is closed or unfocused.

## Focused keys

| Key | Action |
|---|---|
| `j`/`k`, `↑`/`↓` | Move the selection one row |
| `PageUp`/`PageDown` | Move one viewport |
| `Enter` | Live person: focus their terminal. Studio row: make that studio current (focus its remembered person or producer). Closed person: reopens a resumable or fixed seat through the action tracker; other roles get a notice. |
| `Space` | Open or close the three-line drawer under a person or task; on a studio header it folds the whole block |
| `o` | Reopen a closed resumable person (`backlot reopen … --as user`) |
| `v` | Open the Task & Delivery Sheet for the selected studio or task |
| `p` | Peek the person's terminal |
| `y`/`d` | Answer a user-owned prompt; director-owned prompts carry the `dir` tag and no cue |
| `+`/`=` | Raise the selected actor's or task's priority (`backlot priority … --as user`) |
| `-`/`_` | Lower it |
| `s`/`f` | Start the stage / finish the studio — the Call Sheet's own actions |
| `c` | Switch collapsed and expanded |
| `q` | Close the dock |
| `?` | Open `Ctrl+B ?` help |
| `m` | The selected row's context menu — the same list a right-click opens |
| `Esc` | Beside the panes: leave focus — the dock stays put and its borders mute. As an overlay: close |

All other keys are eaten while the panel owns focus, except the prefix, which still arms the
toggle chord — and the four studio keys, which still switch studios and roles with the panel
focused.

### The studio row's menu

The list follows the studio's own state. A live studio offers `Focus producer`, the sheet, the
requirement, `Tell producer…`, `Finish studio…` and `Actor records` — `Start stage` joins it
only while the stage is down, since that is the row which unblocks this studio; stopping the
stage is machine-wide and lives on the machine row's own menu.
`Finish` enters the confirm even while tasks are open, because the command's own refusal names
them, and it draws teal when the studio is ready to finish.

A finished studio has no second finish, so its list swaps that row: while the producer seat is
closed it offers `Reopen producer (o)` in `Finish`'s place and drops `Focus producer` (there is
no terminal to land on). `Reopen producer` runs the same people-reopen path a closed person's own
row uses — a fixed seat reopens without a saved session — and it is the one reopen entry a
finished studio has, since the sidebar and the band show no person rows for it. Once the producer
is live again the seat is already open, so the row reads `Focus producer` and lands on its
terminal like the live menu's own item. The rest of the list stands: the sheet, the requirement
and `Actor records` read the ledger, `Start stage` still stands while the stage is down, and
`Tell producer…` queues mail for a person reopened later. Individual people's reopens stay on
their own closed rows, never in this menu.

A ready-but-unfinished studio reads `state: "finished"` in the document too, so the menu keys on
`ready_to_finish` to keep `Finish` for it. In the band, `f` on a studio's frame arms that finish
only when the frame's studio is `ready_to_finish`; a finished chip, an open-task studio and the
new-studio frame leave `f` a no-op.

## Geometry and `Esc`

The expanded dock's width is fixed by the frame: 38 columns at 160+, 36 at 100+, 28 under.
It sits **beside the panes** whenever the pane surface keeps at least 44 columns after the
sidebar and dock; below that it resolves to the **collapsed strip** — the choice stays
expanded, so widening the window brings the column back. The collapsed strip is always 16
columns: beside the panes from 60 columns up, and a 16-column **overlay** at the pane
surface's right edge under that — the dock never takes the whole surface.

- Beside: `Esc` unfocuses — the borders turn `surface1`; `Ctrl+B y` refocuses, `q`,
  `Ctrl+B Y` or the `×` on the title row closes it.
- Overlay: `Esc` closes; the footer reads `esc back`. An overlay dock's rect is
  reset cell by cell before painting, so nothing underneath shows through.
- `Enter` on a live person or studio focuses that terminal; over an overlay it also
  closes the dock.

Dialogs — the sheet, the New Studio dialog, help — center inside the pane surface left of
a beside dock (or dim the whole buffer when the dock overlays): no dialog ever crosses the
dock's border.

## The blocks

Expanded, the dock's top line is `lot · <n>` — one **block per studio**, in document order,
drawn as a rounded frame (`╭╮╰╯`). The selection is `selection_bg` with an accent `▌` in the
first column; it lands only on actionable rows — decorative lines and block bottoms are
skipped — and borders are honest about focus: unfocused, every block edge is `surface1`.

- The studio id sits in the top border. The first row inside is the **header**: a health
  glyph — `●` live, `◌` stage stopped, `×` offline or a ledger error, `✓` finished — then
  `r<version> · <done>/<total>` and `!<waiting>` in the blocked colour when above zero.
  The current studio's frame is the accent colour, the others a muted surface colour.
- The **owed-step** line under it says where the next step is owed in role names —
  `→ deliver t1 · actor-a` — never a raw `actor-t1-c1` ledger key; the stage-down and
  machine-offline forms say so instead.
- A **person** is one line: mark, name (an actor's shows `· task` and the priority flag),
  a `USER` badge on the user's own role, the read-only `dir` tag on a director-owned
  prompt, and the state word right-aligned. A user-owned prompt adds a cue line
  `! <attention>  [y/d/p]`; a director-owned one gets no cue.
- **Task rows** list work no live actor carries — uncast, dependency-gated (`waits t0`),
  paused (`=`), blocked or at rework cap, and accepted (`✓`) — each as `· <task> <title>`.
  Six or more accepted tasks fold into `✓ <n> accepted` and unfold with `Space` on the
  studio header.
- **Drawers:** `Space` on a person or task opens three dim lines underneath — task and
  priority, the `o`/`v` actions, and `esc`/`space` to close.
- **Shortcuts line:** the last row of the panel, dim. Unfocused: the four studio keys with
  their configured labels (`alt ↑↓ studio  ←→ role`). Focused: the panel's own keys
  (`↑↓ move  m menu  ? keys  esc` — `esc back` over an overlay). Dropped under 8 rows.
- The collapsed **strip** shows each studio as `● <id·machine>` with its `<done>/<total>`
  or `!<n>` — health first, then attention — and one row per active or blocked role,
  `<mark> <name> <task>`; it is never blank while work is alive.

## The Task & Delivery Sheet

`v` opens the sheet on the selected studio — the selected task row's task, a person's own
task, or the studio's overview. It is a centered modal — 92×28 at 160+, 86×28 at 100+,
76×20 at 80, 58×20 at 60, 38×20 at 40 — with three tabs (`Tab` cycles, `1`–`3` jump):

1. **Overview & Casts** — the task's title, status, acceptance criteria and dependency
   chain, then every cast entry.
2. **Deliveries & Reviews** — every delivery with its commit range, the actor-reported and
   the stage-run checks with exit codes and durations, and every review with verdict,
   conclusion and findings.
3. **Actor Records** — the `records --json` read for the studio.

The summary projection paints at once; `status --json` and `records --json` fill in behind
it, fetched off the event loop on the studio's own machine — locally through
`std::env::current_exe()`, remotely through `RemoteRunner`. A reply for a sheet already
closed or reopened is dropped. `o` on a delivery or check line streams its file through
`backlot report show` — a local file for a local studio, the same command remotely — and
`Esc` backs out of the report first. A click outside the frame closes the sheet.

## Person states and attentions

Every open attention carries one class, built by
`backlot_model::attention::AttentionClass::of` from its reason and the ledger's `needs_user`
escalation flag. The class decides what a person row shows; the raw reason never does.

| Reason | Class | Shown as |
|---|---|---|
| `permission prompt` | needs you | `needs you · <what>`; the prompt row keeps `y`/`d`/`p`, and a non-actor's reads `USER` |
| `folder-trust` | needs you | `needs you · <what>` |
| `folder trust refused` | needs you | `needs you · <what>` |
| `cannot resume` | needs you | `needs you · <what>` |
| `launch refused` | needs you | `needs you · <what>` |
| `runner not supported` | needs you | `needs you · <what>` |
| `permission profile not supported` | needs you | `needs you · <what>` |
| `model not supported` | needs you | `needs you · <what>` |
| `effort not supported` | needs you | `needs you · <what>` |
| `config malformed` | needs you | `needs you · <what>` |
| `not a git repository` | needs you | `needs you · <what>` |
| `work area failed` | needs you | `needs you · <what>` |
| `over budget` | needs you | `needs you · <what>` |
| `agent error` | needs you | `needs you · <what>` |
| `owed` | needs you | `needs you · <what>` — the escalated stall |
| `<runner> prep failed` | needs you | `needs you · <what>` |
| `stalled` | stuck | `stuck · <plain reason>` |
| `rate limited` | stuck | `stuck · <plain reason>` |
| `connection error` | stuck | `stuck · <plain reason>` |
| `session id not found` | info | the role's details only, dimmed |
| `inherited XDG_CONFIG_HOME` | info | the role's details only, dimmed |
| `cursor dir was open` | info | the role's details only, dimmed |

A `stuck` episode that escalates — `rate limited` or `connection error` after its auto-retries,
or a stall whose nudge went unanswered for `escalate_after_s` — becomes `needs you`. A reason
the table does not list is `needs you`, so nothing important is hidden. Stage bookkeeping never
changes a person's state and never counts as flagged.

## The cast

In studio mode the sidebar's agents section is the current studio's cast instead (see
[guide.md](guide.md)). Header: `cast · <studio id>` on the left, `<n>/<total>` on the right — the
studio's position in document order. One entry per person, in the summary's order, two rows each:
mark, name and task with the runner right-aligned; then the terminal's title — falling back to
`doing`, then the state word — with the model right-aligned in short form. An attention turns row
two into `! <attention>` in the blocked colour; a failed turn says `last turn failed`. Either takes
the whole row, model included, so it stays readable at the default width. The person
whose terminal is focused gets the active-row background on both rows and an accent `▌` in the
first column. Closed people stay listed, dimmed, after the live ones.

After the people, a `shells` header groups the studio's **shells** — the user's own terminals
opened with `Ctrl+B c` or a split (labelled `backlot.shell=<n>`). Two rows each like a person: a
dim `$` (or the agent status when the shell runs one), `shell <n>`, the program on the right;
then the terminal title and the working directory's last component. The role keys reach them
after the people.

At the bottom one row `others <n> ▸` expands in place to herdr's own agent rows — every terminal
that is neither a studio person, a studio shell, the stage nor Hollywood (what hc-tui, wherdr or
`hc run` created in the same session). Enter or a click toggles it; collapsed by default.

## Mouse

- Click on the title flips collapsed and expanded.
- Click on a block's top border makes that studio current (as `Alt+Down` would).
- Click on a single-line (stacked) block expands it.
- Click on a person focuses them; a click on a row selects it and acts like `Enter`.
- Right-click on a row or the dock surface claims the dock's keys, selects the row and opens
  its context menu.
- The wheel scrolls the body; the scrollbar clicks and drags.
- Dragging the left border resizes the dock between 28 and 48 columns.
- A click outside a focused panel hands focus back to the panes.

## Config

`$XDG_CONFIG_HOME/backlot/config.toml` (default `~/.config/backlot/config.toml`):

```toml
[sidebar]
width = 26   # cast sidebar width in studio mode, clamped to 20-60

[panel]
key = "y"    # the chord tail after the prefix

[dock]
width = 36   # expanded column width, clamped to 28-48

[keys]
previous_studio = "alt+up"
next_studio = "alt+down"
previous_role = "alt+left"
next_role = "alt+right"
call_sheet = "alt+y"            # the direct call-sheet key; "" unbinds it
prefix_call_sheet = "i"         # the prefix-chord tail; "" unbinds it
```

A missing file means defaults. A malformed file means defaults plus one notice carrying only
the line and column — file contents never reach the screen. A `key` that does not parse falls
back to `y` with one notice naming the file; an unparseable studio key falls back to its default
the same way — the invalid value is never echoed. An empty string unbinds a studio key. Under
`--plain` the sidebar follows herdr's `ui.sidebar_width`, as before; a drag on the edge still
resizes it for the session. herdr's own `config.toml` is read exactly as before, except for its
`[theme]`: the same file holds backlot's own ([Theme](../theme.md)).

## Behaviour notes

- The five marks — working (animated), waiting, blocked, lost, closed — pair a distinct symbol
  with a `chrome/status.rs` colour; an unknown `work` reads as waiting.
- Working marks spin about every 120 ms through a one-shot deadline that arms only while a
  working mark is actually drawn; an idle or closed panel wakes nothing.
- Beside the panes only while the pane surface keeps at least 44 columns after the sidebar
  and dock; otherwise the dock resolves to the collapsed strip — beside from 60 columns,
  a 16-column overlay under that — and the pane geometry keeps the closed shape beneath it.
- The document is the local `backlot` summary, schema `backlot/1`. A missing, malformed or
  foreign-schema document reads as `no studios`, never an error. Without a live `backlot-stage`
  terminal the panel shows `stage stopped` and keeps the last document.
- `reopen`/`priority` run `backlot --studio <id> --as user …` off the event loop through the
  injected runner (`std::env::current_exe()` in production, a recorder in tests); a failure
  shows its first stderr line.
- Entering or leaving studio mode may resize the panes once — the sidebar's width switches
  between herdr's configured width and backlot's `[sidebar] width` unless the user dragged it.

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

`Alt+L` (`[keys] focus_lot`) opens or expands the dock and enters the target studio on its priority
landing row (`InsideStudio`: prompt → open ask → needs-you/design/card → active member → `producer`
→ `›`); from `Lot(Studios)` it enters the selected studio, and a second `Alt+L` inside a studio
jumps straight into `Compose`.

## Focus model & inline navigation

Only one region owns keyboard focus (`FocusState`: `Terminal`, `Tree`, `Cast`, `Lot(Studios | InsideStudio |
RowActions | Compose)`, or `CallSheet`):

- **Single focus ring & modebar:** Whenever focus is outside `Terminal`, bordered panes mute with
  `[unfocused · Esc returns]` in their border title and borderless studio headers carry the cue at
  the right end of the top bar (never drawn over terminal content), the modebar shows a clasped
  plaque (`╡ FOCUS: LOT · <studio> ╞` on `accent`, `╡ LOCKED ╞` / `╡ ERROR ╞` on `red`, `╡ RESIZE ╞` on
  `mauve`, plain `--plain`/`source = "herdr"` setups keep plain text), and entered Lot studios draw heavy
  accent rails (`┏`/`┃`/`┗` on outer seam, `┏━┓`/`┣─┫`/`┗━┛` on the studio) with `━ entered ━` in
  the header (`esc studios`). Remembered cursors in non-focused lists show a faint `·` without
  `selection_bg`.
- **Keyboard in Cast (`Ctrl+B a`) & Tree (`Ctrl+B t`):** Focuses the left sidebar sections (`Tab`/`Shift+Tab`
  between them, `Esc` returns to `Terminal`). In Cast, `←`/`→` steps inline action chips on urgent roles,
  `t`/`r` opens targeted Lot compose, and `x` acknowledges an open ask.
- **In-studio Lot grammar (`InsideStudio`):** `j`/`k` and `↑`/`↓` stay bounded within the entered studio;
  `Tab`/`Shift+Tab` jumps non-empty sections (`Header` → `People` → `Tasks` → `Exchange` → `›` Input),
  wrapping across studios (`InsideStudio` follows the target studio);
  `←`/`→` steps into `RowActions` to cycle inverted `« chip »` action pills (`Enter` executes the chip,
  `Esc` returns to the row); `Esc` steps back one level (`Compose` / `RowActions` → `InsideStudio` →
  `Studios` when ≥ 2 studios exist → `Terminal`).
- **Targeted compose & TALK delivery lifecycle:** Pressing `t` or `i` (or `r` on an ask) pre-targets the
  selected role without inserting `/tell <role> ` clutter into the input buffer (`Tab` on an empty buffer
  cycles `producer → cursor role → hollywood`, while `/…` keeps `/verb` completion). Under the newest
  `user` turn in `TALK`, a live status line renders `… sending…`, `… queued · <reason>` (`RoleSummary.queue`
  and client-derived `stage stopped — [s] start`), `✓ delivered · <age>`, or `⠶ thinking…`, clearing once
  a reply lands.

## Focused keys

| Key | Action |
|---|---|
| `j`/`k`, `↑`/`↓` | Move the selection one row |
| `Tab` / `Shift+Tab` | Jump to the first selectable row of the next / previous non-empty section (`Header` → `People` → `Tasks` → `Exchange` → `›` Input), wrapping across studios (compact rows count as header-only) |
| `PageUp`/`PageDown` | Move one viewport |
| `Enter` | Live person: focus their terminal. Studio row: make that studio current (focus its remembered person or producer). Closed person: reopens a resumable or fixed seat through the action tracker; other roles get a notice. Call Sheet `DECISION` row with proposed checks: opens a read-only `CardView` listing `Signing off also activates N check(s):` and every check verbatim (`name`, `run`, `timeout_s`, `expect`). |
| `i` / `/` | Compose on the selected studio's `›` tell input (`/` seeds `/`, `Tab` completes `/verb`). While composing: `↑`/`↓` step per-studio `user` message history (`StudioSummary.exchange` + session submits, saving/restoring any draft, consecutive repeats collapsed), `Enter` sends and rests, `Esc` rests keeping the draft |
| `Space` | Open or close the three-line drawer under a person or task; toggle a `✓ … done` or `□ … closed` fold line; on a studio header, fold/unfold the whole block |
| `o` | On an opening-design row: open `design/v<N>.md` (`backlot report show --file …`) in the `Design` Reader (prepended with `Signing off also activates N check(s):` while unsigned and proposed checks exist). Otherwise: reopen a closed resumable person (`backlot reopen … --as user`) |
| `r` | On an opening-design row or studio awaiting user design reply: focus the studio's `›` producer tell line (or run `backlot design open` when `SuggestSkip`) |
| `a` | On an opening-design row (or inside its `Enter` show view or Design Reader): when proposed checks exist, opens a 3-choice modal listing every check verbatim (`name`, wrapped `run`, `timeout_s`, `expect`) with choices to sign + activate checks, sign without checks (`--no-checks`), or cancel; otherwise opens the confirm modal (`sign off` vs `skip` + frozen file). |
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
| `Esc` | In Compose on `›`: rest on the line keeping any draft. Otherwise beside the panes: leave focus — the dock stays put and its borders mute. As an overlay: close |

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

Two lifecycle rows join that list (W4b). A finished studio also offers **`Free work areas
(prune)…`**: the confirm shows `studio prune <studio> --dry-run`'s plan verbatim, and Enter then
runs the real prune. `Finish studio…` on a studio with no requirement and no tasks asks
`Nothing was built yet (no requirement, no tasks). Finish anyway?` and passes `--force`.

When the TUI is connected to a server (`--server`, `BACKLOT_SERVER`, `[client]`), every studio
management action goes over `plugin.backlot.req`/`res` instead of a local process. The New Studio
dialog's version, `repos` and `runners` probes answer from the hub (the folder browser walks the
**hub's** folders), and its dry run and create travel the queue with `--cast -` on stdin and the
build brief as content — no temp file is written on the client. The delete, prune and restore
dialogs keep their dry-run plan, but the plan now carries a `confirm_token`; the confirm sends
that exact token, and if the studio changed underneath it the server's refusal
(`the studio changed since you reviewed it; run it again`) is drawn in the dialog, where `r` (or
Enter) reviews the plan again. A server create does not chain a separate `stage start`: the
server is the stage.

A live person's menu (`m` on a person) carries **`Restart <name>`**, and a fixed seat — producer,
director, writer — adds **`Recast…`** and drops `Close`, which the command refuses on it. Restart
opens a two-choice confirm (`Resume its session` when the runner keeps a resumable session,
else `Fresh start`); recast steps through runner → model → effort, takes the lists from
`backlot runners --json`, and shows the old → new identity before it runs. A cast fixed seat
that has not started yet renders as a dim **`standby`** row (no activity glyph, ignored by counts)
with `starts when the director plans` for `writer` or `starts with the studio` for `producer` /
`director`; `p` and `⏎` explain that it has not started yet, while `m` still offers **`Recast…`**. An
actor or held seat keeps `Close`.

## Geometry and `Esc`

The expanded dock's width is fixed by the frame: 38 columns at 160+, 36 at 100+, 28 under.
It sits **beside the panes** whenever the pane surface keeps at least 44 columns after the
sidebar and dock; below that it resolves to the **collapsed strip** — the choice stays
expanded, so widening the window brings the column back. The collapsed strip is always 16
columns: beside the panes from 60 columns up, and a 16-column **overlay** at the pane
surface's right edge under that — the dock never takes the whole surface.

- Beside: `Esc` unfocuses — the `▌` row cursor hides (the selected block keeps its `accent`
  border and title chip); `Ctrl+B y` refocuses, `q`, `Ctrl+B Y` or the `×` on the title row closes it.
- Overlay: `Esc` closes; the footer reads `esc back`. An overlay dock's rect is
  reset cell by cell before painting, so nothing underneath shows through.
- `Enter` on a live person or studio focuses that terminal; over an overlay it also
  closes the dock.

Dialogs — studio dialogs, confirms, the sheet, the New Studio wizard, help — center inside the pane surface left of a beside dock (or dim the whole buffer when the dock overlays): no dialog ever crosses the dock's border. Studio dialogs, confirms, and the New Studio wizard carry double Art-Deco borders (`╔══╡ … ╞═══╗`) with `gilt` side rails and `‹ ↵ … ›` primary buttons (`red` border + `◉` on destructive confirms, and `╔═╡ ● <studio> · THE END ╞═╗` on finished-studio toasts), leaving `--plain` and `[theme] source = "herdr"` rounded (`╭─╮`).

## The blocks

Expanded, the dock's top header sits on `stage_well` reading `│ ✦ L O T · <n>` (`┏ ▸ L O T · <n> ━━ esc studios┓`
while entered) — one **block per studio**, in document order:
- Resting blocks draw slim brass frames (`╭─╮│╰─╯`, `├─┤` dividers), and finished studios carry `✦ THE END·<done>/<total>` (stepping down to `✦ END` and `✓ done` on narrower docks, keeping `↺ reopen producer [o]` at the same height).
- Focused at `Studios` (`LotLevel::Studios`), the selected studio promotes to double brass (`╔═╗║╚═╝` in `accent`, `╟─╢` section dividers).
- Entered (`LotLevel::InsideStudio`), the entered studio draws heavy accent borders (`┏━┓┃┗━┛`, `┣─┫` dividers) inside the heavy outer seam (`┏`/`┃`/`┗`).
The row cursor (`selection_bg` with an accent `▌` in the first column) lands only on actionable rows,
skips decorative lines and block bottoms, and stays honest about focus (hidden when unfocused). With
`--plain` or `[theme] source = "herdr"`, rounded `lot · <n>` borders remain unchanged.

- **Opening-design rows & 16-col strip badge:** when a studio has an active pre-signed
  `StudioSummary.design`, its expanded block (and active folded card) shows `◆ design · R<r> · you`
  (`proposal` / `review` / `summary`, or `◆ design · skip suggested · you`), up to 3 `Q1..Q3`
  question previews (or `skip_reason` / `summary`), and the action footer `r reply   o read   a sign`
  (`r discuss  o read  a skip` on `SuggestSkip`). In the 16-col collapsed strip, active design
  discussions render a single-line warm-peach `◆R<r> ?` (or `◆?` on `SuggestSkip` / tight widths)
  without wrapping. Non-active (`Idle` / `Signed`) studios render zero extra rows.
- **Selected-studio folding and compact rows:** in a lot with two or more studios, only the
  selected studio draws in full (the studio holding the `▌` cursor, else the `▸` current studio,
  else the first studio). Any non-current **ready**, **calm**, **finished**, **stage-down**, or
  **offline** studio with no active/alert roles compresses to one borderless row (`✓ gamma-web 5/5 ready`,
  `· calm-dev 1/2 calm`, `✓ omega 4/4 finished`, `◌ stage down`, `× offline`), while active non-current
  studios draw a compact folded card (top border with health/`!n`, one 1-line row per active/alert
  role, bottom border). Inside every expanded studio block, all live, standby, lost, and waiting roles
  always render on their own rows in group order (`producer`, `director`, `writer`, actors) with thin
  `├──┤` group rules regardless of whether the left Cast sidebar is open, while closed actors stay
  folded behind `□ <n> closed`. Stepping into any folded or compact studio or clicking its row selects
  and unfolds it while keeping it scrolled into view. A single-studio lot never folds, manual `Space`
  collapse (`╶` stacked row) or expand overrides still hold, and unselected studios fall back to
  one-line `╶` stacked rows when blocks exceed the body height. Talk budgets go to the selected full
  block only. When body height allows, one blank spacer row separates studio blocks and compact-row
  groups (dropping first under height pressure before any content or folding).

- Every studio name in the Lot draws in **bold** `palette.text` (top borders, compact rows, stacked rows,
  and the strip). The studio whose workspace is currently shown (`▸`) sits in a 1-cell-padded subtle
  capsule (` <id> ` / ` ▸ <id> ` on `elevation.raised_row`, or bold + reverse on 16-color terminals);
  when that block is also selected in a focused Lot, the `accent` selection chip wins without stacking
  two highlights. The top bar studio breadcrumb uses the same raised capsule, and the left Cast
  header (`<studio>  <n>/<total>`) draws in bold `palette.text`. The first row inside is the
  **header**: a health glyph — `●` live, `◌` stage stopped, `×` offline or a
  ledger error, `✓` finished — then `r<version>·<done>/<total>`, `· <live>/<cap>` only when live
  actors exceed zero (idle `0/4` is omitted), and `!<waiting>` in the blocked colour when above zero.
  When focused, selected rows on stacked, compact, and collapsed strip lines keep bold `accent`.
- The **owed-step** line under it says where the next step is owed in role names —
  `→ deliver t1 · actor-a` — never a raw `actor-t1-c1` ledger key; the stage-down and
  machine-offline forms say so instead.
- The **next-step** line under that one comes from the studio's own state:
  `· next: tell the producer what to build` (no requirement recorded and no brief queued; Enter on the studio row or
  `t` opens the producer tell input), `· next: director reviewing` (tasks in review), or
  `· next: finish (f)` (every task accepted). A studio started with `--ask`/the wizard's build
  field already has its brief queued as the producer's first `tell`, so it moves past this state
  on its own. Any other state draws nothing. The Hollywood frame carries the same line on its
  blank row.
- Role groups are split by a thin `├──┤` rule in the frame's colour — the producer, then
  the director, then the writer, then all actors together; an empty group draws no rule,
  and a folded or stacked block follows the same grouping.
- A **person** row is always **one line** in the Lot dock, sharing Cast row 1's unified
  `InlineState` formatter (`RoleBadge` `Ⓟ`/`Ⓓ`/`Ⓦ`/`Ⓐ` + priority flag + name/task + aligned semantic
  mark, state word, optional age, and CJK-safe truncated `· <detail>` tail). Runner and model
  metadata (`runner · model · effort`) shows in the center studio header (`HeaderKind::Role`) and on an
  indented sub-line under each open role in the left Cast (dropped only in the Lot dock), and working
  roles keep all details inline without a separate `working <dur>` subline. When narrowed or CJK-heavy,
  the state word keeps its alignment and name/detail truncate at character boundaries with `…`.
- **Task rows** list work no live actor carries — uncast, dependency-gated (`waits t0`),
  paused (`=`), blocked or at rework cap, and accepted (`✓`) — each as `· <task> <title>`.
  Two or more accepted unrepresented tasks fold into `✓ <n> done · <ids>` (ids fitted to the width
  with `…`) and toggle open/closed with `Space` or `Enter` on the fold line.
- **Closed seats fold:** when closed roles exist in a full block, they fold by default behind one
  dim selectable row `□ <n> closed · <names>` (role names in first-seen order, `×k` when repeated,
  fitted to the width at a name boundary with `…`). `Space` or `Enter` toggles it open or closed per
  studio; when open, closed seats render underneath and `o` reopens a resumable seat as usual.
- **Drawers:** `Space` on a person or task opens three dim lines underneath — task and
  priority, the `o`/`v` actions, and `esc`/`space` to close.
- **Footer & header hints:** when unfocused, the Lot dock leaves its bottom row blank on `elevation.surface`
  (no static `alt PgUp/PgDn studio  ↑↓ role` chrome) and shows only `<focus_lot> focus` in its header
  without repeating Call Sheet keys. When focused, it swaps the header hint to `esc leave` (`esc back`
  over an overlay) and draws a `border_muted` hairline (`─`) above up to 3 structured verbful hint
  pairs via `put_hint_bar` (for example while composing on `›`: `⏎ send  ↑↓ history  esc done`).
  Both hairline and footer hints drop below 8 rows. Unfocused `›` compose inputs stay on `elevation.surface`,
  lifting to `raised_field` only while actively composing.
- **Restart to update:** while a source says a newer `backlot` is installed than the
  running client, one dim line closes the lot:
  `backlot <new> installed — restart the client to update (q, then backlot)`. The sources
  are the local stage's published `backlot_version`/`backlot_build` and any installed
  `backlot --version` the six-hour machine probe answered. Versions compare as strict
  semver; equal versions fall back to the published build id when both sides have one.
  An older version never draws the line, and an unparseable side is ignored rather than
  guessed at.
- The collapsed **strip** shows each studio as `● <id·machine>` with its `<done>/<total>`
  or `!<n>` — health first, then attention — and one row per active or blocked role,
  `<mark> <name> <task>`; it is never blank while work is alive.

## The Task & Delivery Sheet

`v` opens the sheet on the selected studio — the selected task row's task, a person's own
task, or the studio's overview. It is a centered modal — 92×28 at 160+, 86×28 at 100+,
76×20 at 80, 58×20 at 60, 38×20 at 40 — with three tabs (`Tab` cycles, `1`–`3` jump):

1. **Overview & Casts** — the task's title, status, acceptance criteria and dependency
   chain, then every cast entry; a finished studio's overview closes with a centred
   `───══╡ ✦  T H E   E N D  ✦ ╞══───` intertitle on an existing blank row without adding a row.
2. **Deliveries & Reviews** — every delivery with its commit range, the actor-reported and
   the stage-run checks with exit codes and durations, and every review with verdict,
   conclusion and findings.
3. **Actor Records** — the `records --json` read for the studio.

The summary projection paints at once; `status --json` and `records --json` fill in behind
it, fetched off the event loop on the studio's own machine — locally through
`std::env::current_exe()`, remotely through `RemoteRunner`. A reply for a sheet already
closed or reopened is dropped. `o` on a delivery or check line streams its file through
`backlot report show` — a local file for a local studio, the same command remotely, including `.md` artifacts under `design/` and `plan/` — into the **Lot Reader** (`j`/`k`/`Space`/`b`/`Ctrl+D`/`Ctrl+U`/`g`/`G` scroll, `←`/`→`/`[`/`]` step files or jump across `## R<r>` / `### §R<r>.<W|D|P|U>` anchors on `design/` documents, `t`/`i` compose inline sharing the dock's `›` draft and `↑`/`↓` history plus `Tab /verb`, `o` focuses/reopens, `p` peeks read-only, `y` copies raw text, `?` opens help, `Enter`/`Esc`/`q` close back to the sheet). In studio chrome at 64+ columns, the Reader draws the stepped double brass intertitle frame (`╔══╤═` / `║  │` / `╟──┴` / `╟──┬` / `╚══╧═`) — collapsing below 64 columns to a plain double frame (`╔═ … ═╗`, `║`, `╟─╢`, `╚═╝`) — renders `##` and `### §R<r>.*` headings as centred chevron intertitles (`───══╡ ‹✦› … ‹✦› ╞══───` and narrower steps), and replaces quote/`Q1..Q3` margin markers with a `gilt` `║` bar while keeping chrome height and prose measure unchanged (`--plain` and `[theme] source = "herdr"` keep the classic rounded frame). Opening a studio opening design document renders the `Design` Reader variant (`✦  D E S I G N   v N  ·  R r  ·  <studio>` when it fits or compact `✦ DESIGN v<N> · R<r> · <studio>` when tight, Row 2 phase badge, round, and file path); while in `WaitingUser` or `SuggestSkip`, `r` replies to `producer` and `a` confirms design sign-off (`backlot design sign [--skip]`), while finalized `design/v<N>-final.md` (`Signed`) documents omit the `a sign off` chip. A click outside the frame closes the sheet.

**Lot Reader from the dock:** `Enter` or a click on any `✦` / `›` exchange line (and `m → Records`) opens the same adaptive Reader card beside the undimmed Lot Dock (`▌` tracks `←`/`→` or `[`/`]` stepping in place, and clicks on beside `✦`/`›` lines switch in place). Action strips adapt cleanly down to compact 80-column chips (`t tell`, `o focus`/`o reopen`, `p peek`, `v sheet`, `y copy`, `? keys`; classic `[k] label` under `--plain`), while `y` copies raw UTF-8 without answering prompts and `d` stays unbound. Pressing `t`/`i` or clicking `t tell` opens the inline composer backed by the studio's `›` draft and `↑`/`↓` history (`Esc` leaves compose keeping any draft; a second `Esc` closes the Reader with the draft still mirrored in the dock).

## Person states and attentions

Every open attention carries one class, built by
`backlot_model::attention::AttentionClass::of` from its reason and the ledger's `needs_user`
escalation flag. The class decides what a person row shows; the raw reason never does.

| Reason | Class | Shown as |
|---|---|---|
| `permission prompt` | stuck | `stuck · <what>` when the director handles an actor's; `needs you · <what>` when `needs_user` is set (fixed role, sensitive, or escalated), keeping `y`/`d`/`p` and `USER` on non-actors |
| `outward action` | info | the role's details only, dimmed — a refusal the role's own turn moved past; no counts, no notice |
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
| `auth error` | needs you | `needs you · <what>` |
| `agent error` | stuck | `stuck · <what>` when an actor's goes to the director; `needs you · <what>` when `needs_user` is set (fixed roles) |
| `owed` | needs you | `needs you · <what>` — the escalated stall |
| `<runner> prep failed` | needs you | `needs you · <what>` |
| `stalled` | stuck | `stuck · <plain reason>` |
| `looping` | stuck | `stuck · <plain reason>` (same role hit `>= 5` identical non-probe guard refusals in 10m; reviewer notified once per window, never sets `needs_user`) |
| `question` | stuck | `stuck · question` (or `needs you` when `needs_user` is set, e.g. on producer) |
| `rate limited` | stuck | `stuck · <plain reason>` |
| `connection error` | stuck | `stuck · <plain reason>` |
| `session id not found` | info | the role's details only, dimmed |
| `inherited XDG_CONFIG_HOME` | info | the role's details only, dimmed |
| `cursor dir was open` | info | the role's details only, dimmed |
| `hooks unavailable` | info | the role's details only, dimmed |
| `no progress` | info | low-noise note (`2 × stall_limit_s` active coding time without work-area changes; routed to director, never counts toward needs-you or stops the studio) |

A `stuck` episode that escalates — a director-handled `permission prompt` when the director is
blocked, not live, or unanswered past ten minutes (`default_stall_limit_s`), `rate limited` or
`connection error` after its auto-retries, or a stall whose nudge went unanswered for
`escalate_after_s` — becomes `needs you` through `attention.escalated` on the same attention,
so one episode stays one row and Hollywood hears once. A reason
the table does not list is `needs you`, so nothing important is hidden. Stage bookkeeping never
changes a person's state and never counts as flagged.

**What reaches the person, and why.** Only reasons where the person alone can act are `needs you`
and count toward the person's needs-you list, the lot pill and notifications: a permission or
trust prompt the stage or director will not answer (`permission prompt` with `needs_user`,
`folder-trust`, `folder trust refused`), a question addressed to the user, `cannot resume`,
`launch refused` and the `<runner> prep failed` / `not supported` / `config malformed` /
`not a git repository` / `work area failed` launches only the person can fix, `over budget`,
fixed-role `agent error`, and the escalated `owed` step. `permission prompt` and `agent error`
on an actor start `needs_user: false` (`stuck`, handled by the director) and therefore do not
increment `StudioSummary.waiting` until escalated. `outward action` is `info`: the guard stopped a role's command and told the role to
`backlot ask` if it still needs it, so the refusal stays on the studio's attention list and the
role's row but never counts as the person's to-do — it clears as `role moved on` once the role
writes anything else to the ledger after it (or the stage delivers it new work). A carded
outward push or `gh pr create` is unchanged: it surfaces as its own open card.

A person shows exactly one state, in this priority:

1. `needs you · <what>` — a user or director prompt as before (the prompt row keeps `y`/`d`/`p`
   and a non-actor's word is `USER`), or a `needs you` attention that is not a prompt (`■` red).
2. `asks you · <question>` — the role's newest explicit `tell user --ask` the user has not
   answered (`?` peach). A plain `tell user` (including `tell user --note`) is a report in the
   conversation, never a question; the CLI refuses unflagged `tell user` messages from studio
   roles that contain `?` or `？` outside code spans, fenced blocks, or URL query strings so a
   question cannot drop silently into a plain report without `--ask` or `--note`.
   The answer is a `tell` back to that role or any later ledger action the role records
   itself — a revised requirement, a report, a delivery, a cast, a review, a new tell — because
   the person often answers in the role's terminal. `ask.acknowledged` is the user's explicit
   receipt without an answer. Stage-written events never answer it; the
   Dock records no read state, so merely opening the message does not either.
3. `stuck · <plain reason>` — a `stuck` attention (`!` peach).
4. `working · <doing>` — the watch pass reads the screen working.
5. `waiting` — anything else that is live and not doing work; a held role reads its
   `waiting for a <runner> slot` detail, and a role that has sat at its prompt past 60 s reads
   `idle <n>m` (`backlot status` shows the same word).
6. `ready` — live, idle and nothing assigned (`◦`).
7. `standby` — a cast fixed role that has not started yet (no activity glyph, dimmed, ignored
   by counts; `writer` shows `starts when the director plans`, `producer` and `director` show
   `starts with the studio`).

An `info` attention never appears in this list; it shows only in the role's details (the `Space`
drawer and the sheet's studio overview), dimmed.

## The needs-you list

One function builds the list from every machine's document, in this order: user-owned prompts;
upgraded-ledger / explicitly halted studio rows (`! HALTED`, red single-row blocking entry whose `Enter` opens a read-only `CardView` showing who halted and the reason);
roles with a `needs you` attention or an unanswered question; Hollywood with a user-owned prompt
(`HOLLYWOOD asks: <attention_detail>`, taking the same `y` allow once / `d` deny / `p` peek /
Enter actions as its live row) or a blocked screen with no prompt (`HOLLYWOOD blocked`); studios
with no requirement; stopped stages and unreachable machines with live studios; open cards;
studios ready to finish. The Call Sheet's waiting rows, the lot pill's urgent count and the band's
calm row all read this one list. `✓ nothing waiting on you` shows only while it is empty; there is
no separate `!N flagged` suffix — a `stuck` role shows on its own row and never counts here.

The band lists every actionable item in that needs-you list on its own row — prompts,
needs-you/asks roles, Hollywood waits, design decisions, new studios, stopped stages, open cards,
and outward cards are never folded into a studio's `READY` row or `· +n`, so actionable rows on a
studio always come before its `READY` row and the header count matches what is listed. An ASKS row reads
`? ASKS <studio> · <role> · <age> — <first sentence>` (truncated to width; peek/read shows the
whole turn), offers `r` reply / `x` acknowledge / `p` peek / Enter, and paints action chips only
on the selected row. `r` composes a tell back to the role; `x` records `ask.acknowledged`.
Refusals caused by a race — already answered, already cleared, no longer waiting — are plain
fading text rather than raw sequence-number errors.

## Notifications

Studio events reach the person through the client's own notification pipeline — the same settle,
queue and TTL a pane alert uses, with the same `[ui.sound]` and `[ui.toast]` config. Each event
is derived from the `backlot` documents (the same needs-you list above), keyed so it fires
**once per ledger identity** and never again — a card by its `c<seq>`, a reply by its ledger
`seq`, a role's question or prompt by the seq behind it, a studio state by `(machine, studio)`:

| Event | Text |
| --- | --- |
| A role's unanswered question | `producer asks: <question>` |
| A user-owned prompt on a role | `<role> needs you: <command>` |
| A needs-you attention on a role | `<role> needs you: <detail>` |
| A user-owned prompt on Hollywood | `hollywood needs you: <command>` |
| A new Hollywood reply | `hollywood replied: <headline>` |
| An open card | `card waiting: <action>` |
| A studio with every task done | `acme-shop is ready to finish` |
| A stage that stopped with live studios | `stage stopped on host-a` |

- The **first document after start seeds silently**: what predates the client is not news, and a
  reconnect resets the same way, so a restart never replays old events.
- A target the person is already looking at is **suppressed**, exactly as a watched pane is: a
  focused role terminal, `bl-hollywood`, the Call Sheet on that card, the current studio or its
  focused person, or the dock on that stopped-stage row.
- The toast's **key (`prefix+o`) or a click opens the row's target** — the role's terminal,
  `bl-hollywood`, the Call Sheet on that card, the studio, or the dock's `stage stopped` row. A
  jump selects, it never acts: the stopped-stage row lands selected with the keys on the dock so
  `s` is the person's own press.
- A blocked Hollywood with no prompt, an unreachable machine and a studio with no requirement are
  not events — they have their own rows in the needs-you list.
- `[ui.toast] studio = false` in herdr's config (default `true`) turns the studio events off; the
  pane alerts are unaffected. `--plain` is herdr's screen exactly and emits nothing.

## The cast

In studio mode the left sidebar deduplicates active live studios from the `machines` workspace tree
(retaining non-studio workspaces, `stage down`, `finished (<n>)`, `Archived (<n>)`, and the stage
row `backlot <v> · ● ok`), places a clickable `? keys` affordance at bottom-left beside `«`, and
shows the current studio's cast instead of Herdr's plain agents list (see [guide.md](guide.md)).
The cast never sits blank while studios exist: the current studio's cast shows when one is known;
with none current but exactly one live studio in the lot, that studio's cast shows and the header
names it; with more than one live studio and none current the body is one dim line,
`select a studio (Enter on it)`. Header: `<studio id>` (or `<studio id> · <machine>`) in bold `palette.text` on the left, `<n>/<total>` in bold `palette.text` on the right — the studio's position in document order.

One entry per person, in the summary's order: row 1 uses the shared `InlineState` formatter as the
Lot dock (`RoleBadge` + mark, name/task, and aligned semantic state word / age / `· <detail>`,
truncating CJK safely with `…`), and open/in-use roles (`producer`, `director`, `writer`, and
non-closed actors) show an indented second line in quiet `palette.subtext0` carrying
`runner · model · effort` (omitting missing fields cleanly and cutting with `…` at sidebar width;
under height pressure unfocused roles fall back to one line first so the focused role keeps its
model line longest, and no role is ever hidden for a model line). An `info` attention stays hidden
from the row. The person whose terminal is focused gets the active-row background and an accent `▌`
in the first column. In studio mode the Cast top rule renders as `──╡ ✦ CAST ╞────` (`━━╡ ▸ CAST ╞━━━━` in `accent` while focused) in the same row, and closed people fold behind `── · closed (n) · ───────` (`┄ closed (n) ┄` in `--plain` / `source = "herdr"`), staying strictly one line with no model row.

After the people, a `shells` header groups the studio's **shells** — the user's own terminals
opened with `Ctrl+B c` or a split (labelled `backlot.shell=<n>`). One line each: a dim `$` (or the
agent status when the shell runs one), `shell <n>`, the program, and the terminal title/cwd. The
role keys reach them after the people.

At the bottom one row `others <n> ▸` expands in place to herdr's own agent rows — every terminal
that is neither a studio person, a studio shell, the stage nor Hollywood (what hc-tui, wherdr or
`hc run` created in the same session). Enter or a click toggles it; collapsed by default.

## Mouse

- Click on the title flips collapsed and expanded.
- Click on a block's top border or folded role row selects and unfolds that studio (`Enter` on the
  header makes it current).
- Click on a single-line (stacked) block expands it.
- Click on a fold row (`✓ … done` or `□ … closed`) toggles it open or closed.
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
previous_studio = "alt+pageup"
next_studio = "alt+pagedown"
previous_role = "alt+up"
next_role = "alt+down"
focus_lot = "alt+l"
focus_tree = "prefix+t"
focus_cast = "prefix+a"
call_sheet = "alt+y"            # the direct call-sheet key; "" unbinds it
prefix_call_sheet = "i"         # the prefix-chord tail; "" unbinds it

[client]
# server = "host-a"             # default server target (--server and BACKLOT_SERVER override it)
# session = "backlot"           # default session when `server` omits `/<session>`
```

A missing file means defaults. A malformed file means defaults plus one notice carrying only
the line and column — file contents never reach the screen. A `key` that does not parse falls
back to `y` with one notice naming the file; an unparseable studio key falls back to its default
the same way — the invalid value is never echoed. An empty string unbinds a studio key. Under
`--plain` the sidebar follows herdr's `ui.sidebar_width`, as before; a drag on the edge still
resizes it for the session. herdr's own `config.toml` is read exactly as before, except for its
`[theme]`: the same file holds backlot's own ([Theme](../theme.md)).

## Behaviour notes

- The marks — working (animated), waiting, ready, held, needs you, asks, stuck, failed, lost,
  closed, standby — pair a distinct symbol with a `chrome/status.rs` colour; an unknown `work` reads as
  waiting.
- Working marks spin about every 120 ms through a one-shot deadline that arms only while a
  working mark is actually drawn; an idle or closed panel wakes nothing.
- Beside the panes only while the pane surface keeps at least 44 columns after the sidebar
  and dock; otherwise the dock resolves to the collapsed strip — beside from 60 columns,
  a 16-column overlay under that — and the pane geometry keeps the closed shape beneath it.
- The document is the local `backlot` summary, schema `backlot/1`. A missing, malformed or
  foreign-schema document renders an empty Lot plaque (`─── ✦ ───` / `no studios on the lot` /
  `n new studio · alt+y hollywood`, or compact `✦ no studios · n new` on short panels), never an
  error. Without a live `backlot-stage`
  or `backlot-server` terminal the panel keeps the last document and shows `stage stopped`; when
  `backlot-cat` is also present (server mode), it shows the read-only start command
  (`systemctl --user start backlot-server.service` or `backlot server start --session backlot`)
  instead of starting anything itself.
- The left machines tree is not only spaces: under each machine it also draws the studios its
  ledger lists when the document cannot speak for them (`stage down`, dim, Enter starts the
  stage), the machine's folded `finished (n)` group, and one `unreachable` line for a machine
  that cannot be reached at all. The stage-down row puts the studio name first and tags it a
  short `down`, so a 10–12 character id fits whole at the default sidebar width. The ledger
  list comes from `backlot studio list --json`, run locally or over `terminal.run` for a remote,
  cached and refreshed on a stage change or every 30 s at most. When a machine's
  `studio list --archived --json` is non-empty the tree adds one dim `Archived (n) …` row after
  the finished group; Enter opens the archived list (name, stamp, repo, requirement) and its
  `Restore as <name>` dialog, which runs `studio restore` with `--dry-run` first. The archived
  list is cached per machine on the same 30 s clock. The tree scrolls to the machine
  row of the studio the selection lands on. While the Lot is focused and any row of a studio
  is selected, the tree quietly marks that studio's endpoint row and `backlot` server row under it
  with an accent `▎` left bar and bold server line (without scrolling the tree if off-screen),
  clearing the mark when the Lot loses focus.
- `Ctrl+B t` gives the tree the keyboard — `↑`/`↓` move the highlighted row (drawn with the
  accent `▌`), `Enter` runs the row's click action, `m` opens its context menu, `Esc` returns the
  keys to the terminal. The whole tree's surfaces are also in `Ctrl+B ?`'s `machines` group.
- `Ctrl+B ?`'s `lifecycle` group lists `Restart <role> · <studio>` for every live role,
  `Recast <role> · <studio>` for the fixed seats, `Free work areas · <studio>` for a finished
  studio and `Archived studios · <machine>` once a machine's archive is non-empty; the filter
  reaches them by role, studio or machine name.
- The stage log is read-only: when its terminal holds the focus, typed keys are dropped and the
  mode bar reads `stage log — read only`. A stage start never moves the focus to it.
- `reopen`/`priority` run `backlot --studio <id> --as user …` off the event loop through the
  injected runner (`std::env::current_exe()` in production, a recorder in tests); a failure
  shows its first stderr line.
- Entering or leaving studio mode may resize the panes once — the sidebar's width switches
  between herdr's configured width and backlot's `[sidebar] width` unless the user dragged it.
- Opening design cards and the Design Reader read the additive `design` (`DesignSummary`)
  projection from `backlot/1` whenever `design_gate = "required"` and `phase != "idle"`, routing
  `waiting_user` / `suggest_skip` decisions to the warm `?N` bucket (never blocking `!B`),
  rendering `+N check(s)` on the Call Sheet `DECISION` row and opening verbatim proposed checks on
  `Enter`, loading `design/v<N>.md` / `design/v<N>-final.md` via `report show --file ...`
  (`[`/`]` / `◀`/`▶` jump `## R<r>` / `### §R*` headings), and dispatching `a` / `r` via
  `backlot design sign [--skip]`, `tell producer`, or `backlot design open` — locally or over
  `plugin.backlot.req` (`design-sign`, `design-start-round`, `tell`); see
  [design.md](design.md#opening-design-discussion) and [requests.md](requests.md). Also the Task &
  Delivery Sheet shows any `expect_failed` detail on `task.checked` records alongside exit codes.

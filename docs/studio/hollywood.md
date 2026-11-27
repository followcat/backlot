# Hollywood — the overseer

Hollywood is one agent above all studios on a core: the user's assistant for the whole lot. It
reads every studio's ledger, tells you what needs you, reports on any or all studios on request,
and manages studios through **cards** — proposals you confirm. It is not a studio and it is not a
role inside one: it has its own ledger beside `studios/`, in the same format, and studio lists,
limits and studio rules never see it.

This document covers what exists today (H1–H4: the model, the command line, the stage's live
layer, and the floating layer in the TUI) and marks what lands with real agents (H5).

## The layer and its key

The bar has three states, and it is always a float: it is drawn *over* the screen — after the
panes, the cast sidebar and the studio panel — and never takes a row or a column from the pane
layout, so toggling it resizes nothing.

- **Hidden** draws nothing and consumes no key. In studio mode the bar exists whether or not a
  `[hollywood]` table does (U7); without one it is the no-agent bar, and `backlot --plain` has
  no bar at all, so its screen is exactly the herdr screen.
- **Collapsed** is the **bucketed lot pill** at the right end of the chrome row (`HeaderKind::Role`
  also shows `<studio> › <badge> <mark>  <runner> · <model> (<effort>)` beside it on the left):
  a reserved clickable segment displaying studio count plus semantically bucketed waits derived
  from the single needs-you list (`✦ <S> · !<B> · ?<A> · <C>c · ✓<R> ready · alt+y`, shrinking
  through `Medium` and priority `Tiny` tiers when narrow). The pill paints red **only** when
  `blocking > 0` (`!B`: user prompts, needs-you/blocked roles, stopped stages, unreachable machines,
  missing requirements); otherwise it uses `elevation.raised_row` with `teal`/`peach`/`accent`
  segments. Clicking a bucket segment jumps straight to that `WaitBucket` (`Blocking`, `Asks`,
  `Cards`, `Ready`) in the expanded Call Sheet.
- **Expanded** is the **Call Sheet** — a focus-driven docked band dropped from the top over the
  pane + dock width (`surface`), capped at half the frame height (`12..=h/2`, or `h.min(16)` on a
  mobile width), ending in a single `▀▀` bottom perforation edge with no top perforation row and
  no external drop shadow (a beside Lot dock starts directly below `▀▀`, and a click below `▀▀`
  is outside the sheet). Zones appear in fixed top-to-bottom **screen order**:
  1. **Title** (`1` row): `✦  H O L L Y W O O D` (`✦ HOLLYWOOD` under 100 columns) with the agent's
     square, state and `runner / model / effort` on the left, and Lot-pill-matching bucket counts
     (`<S> studios · !B · ?A · Cc · ✓R ready`, plus `{top}–{bot}/{N}` when waiting overflows) on
     the right.
  2. **WAITING** (omitted completely when nothing waits — no `WAITING ON YOU` rule and no calm filler
     row): one needs-you list across every machine in urgency order — user prompts (`Prompt`),
     needs-you/asking roles (`Role`), Hollywood prompts/blocks (`Wait::Hollywood`), opening-design
     decisions (`Wait::Decision`, `◆ DECISION`, warm peach `?A` asks, round/`skip suggested`, summary,
     up to 3 `Q1..Q3` items), no-requirement studios (`Wait::NoRequirement`), stopped stages / offline
     machines (`Stage`), open cards (`Card`), outward cards (`Outward`), then ready studios
     (`Ready`). Unfocused, each item is **1 line** (no detail lines, no action chips). When
     `WAITING` is focused, a `palette.accent` `▌` gutter runs down column 0 of the entire zone and
     only the selected item expands (`why:`, `Q1..Q3`, or inline `finish studio? y/n` + action
     chips on the right). If tight height sheds the Title row while waiting items scroll,
     `{top}–{bot}/{N}` moves onto the first waiting row.
  3. **HOLLYWOOD**: unfocused, shows the newest **1** talk line (clipped with `…`, ending in
     `<age> · <N> earlier` or `<age>`) plus the `tell hollywood…` compose field on `raised_field`
     (or `⏎ hollywood setup` when unconfigured). When `HOLLYWOOD` is focused, draws the
     `── HOLLYWOOD ──` zone rule (`<N> earlier · ↵ read` on the right when earlier turns fold),
     up to 8 newest talk lines with their 2-line CJK-aware wraps, the optional live progress /
     non-duplicate headline row, and compose (dropping the zone rule first when space is scarce
     while keeping `<N> earlier` inline).
  4. **STUDIOS**: unfocused, renders a **1-row chip strip** (`STUDIOS N  [▸acme-shop ✓ 3c] … [+ new]`)
     marking the active main-pane studio with `▸` and appending per-studio `!B`/`?A`/`Cc` or live
     `done/total`, with `[+ new]` pinned at the right and `‹`/`›` sideways scrolling on the rest.
     When `STUDIOS` is focused and space permits, it expands to the centered
     `▄▄ ▄▄ …  STUDIOS N  … ▄▄ ▄▄` perforation header plus frames — `7` rows full (role badge + next
     step + cast dots), `5` rows compact (dropping the role badge and next/blank row), or `3` rows
     micro (border + `state · round · done/total` + border) — falling back to the focused 1-row
     strip (`▌` + accent `STUDIOS N`) when fewer than `4` rows remain.
  5. **Footer** (`Hints` row + `▀▀` bottom edge).

  **Two-phase height rule:** every zone is laid out unfocused first; if it exceeds the cap,
  chrome and unfocused lines shed in order (`hints` → `▀▀` → unfocused talk → title → unfocused
  studio strip → scroll waiting window keeping selection, leaving compose and the selected wait
  intact). Only the currently focused zone then expands into remaining rows (`cap - base`),
  never shrinking another zone's unfocused lines (an inline `finish studio? y/n` confirm also keeps
  its 1-line ask visible when armed from `STUDIOS`).

The start policy is **`collapsed`** by default (`[hollywood] start` — `collapsed`,
`when_waiting`, or `expanded`), applied once the first settled `backlot` read lands.
`when_waiting` opens expanded only while the urgent count is above zero — and again on the edge
up — and a studio that arrives later never pulls the bar open. Neither the card nor the bar
draws before that first read settles (two seconds at most), so a client with studios never
flashes the welcome card.

The key is **`alt+y`**, a direct key with no prefix, plus the `Ctrl+B i` chord tail. From Hidden
or Collapsed it lands directly on the first blocking wait (`Resting`) whenever any blocking item
waits (`!B > 0`), and opens straight into Compose (`›`) when `blocking == 0` and `[hollywood]` is
set (or wait/frame landing when unconfigured); from Expanded it collapses keeping any
draft. Waiting rows always remain visible above Compose, `Esc` in Compose rests on `›` keeping the
draft, and a second `Esc` in Resting collapses it. Toast jumps (`Ctrl+B o` or click) still land on
their named target in Resting, and clicking a pill bucket jumps directly to that bucket's first item.
`[keys] call_sheet` rebinds the direct key and `[keys] prefix_call_sheet` the
chord tail — an empty string unbinds that slot; `[hollywood] key` fills whichever slot `[keys]`
leaves unset (a direct binding like `alt+y`, or `prefix+<key>` for a chord). It is
bound in studio mode with or without a `[hollywood]` table; under `--plain` it is not bound, so
Alt+Y reaches the focused program as always. A key value that does not parse, is a bare printable
key, or collides with one of the studio keys (`previous_studio`, `next_studio`,
`previous_role`, `next_role`, `focus_lot`) or the panel chord falls back to `alt+y` with a
notice that names the file and never echoes the value. Known costs, said plainly:

1. While bound, **Alt+Y no longer reaches the program in the pane**. Readline and Emacs use `M-y`
   for yank-pop; if you use it, rebind (`key = "prefix+h"`) or unset the layer.
2. On a terminal **without the kitty keyboard protocol**, Esc followed by `y` inside the escape
   window (150 ms while the mouse is captured) reads as Alt+Y. Rare in practice; the same rebind
   fixes it.
3. On **macOS**, the terminal must send Option as Meta.

The studio panel keeps `prefix+y` to toggle into `Studios` level, and **`alt+l`** (`[keys] focus_lot`,
`""` unbinds) switches straight into the selected studio frame on its priority landing row (`InsideStudio`,
with a 2nd press jumping to `Compose`). `Ctrl+B t` (`[keys] focus_tree`) and `Ctrl+B a`
(`[keys] focus_cast`) focus the left Tree and Cast sections (`Tab` swaps between them). All non-modal
region chords (`Alt+L`, `Alt+Y`, `Ctrl+B y/t/a`) cross cleanly between regions without trapping
(`docs/keys.md`). Under `--plain`, `Alt+L` and `Alt+Y` fall through to the program.

Inside the expanded bar: `Tab` / `Shift+Tab` cycles screen order — `Waiting` → `Hollywood`
(compose armed) → `Studios` → `Waiting` — skipping empty zones (`Shift+Tab` reverses); in
Resting, `←` `→` (or `h` `l`) move between the studio chips/frames (new-studio included, scrolling
uneven strip chips while keeping `[+ new]` pinned) or action chips, `↓` `↑` (or `j` `k`) cross zone
boundaries (`Waiting ↔ Hollywood ↔ Studios`) smoothly with focus following selection, `PageUp` /
`PageDown` scroll the waiting list, `1`–`9` select that studio chip/frame (focusing and expanding
`STUDIOS`) and `0` selects `[+ new]`, and `Enter` activates the selection. Each waiting row has its own keys: on a
**prompt** row `y` answers `allow` and `d` answers `deny` (`backlot approve --person <role>
--decision <d>` on the prompt's own machine), `p` peeks at the role's live screen read-only;
on a **stage** row `s` runs `backlot stage start` on that machine, and an offline machine's
`Enter` reports the watcher's last read; on a **card** row `a` applies, `x` cancels and `s`
shows the card (`backlot card show` in a read-only view); on a **ready** row `f` asks
`finish studio? y/n` inline, and `y` runs `backlot studio finish` on the studio's own
machine; on a **decision** row `o` opens `design/v<N>.md` in the `Design` Reader (`backlot report show --file …`), `r`
focuses producer reply (or runs `backlot design open` on `SuggestSkip`), and `a` opens the shared confirm
dialog naming mode (`sign off` vs `skip`) + frozen file (`design/v<N>-final.md` / `design/v<N>.md`)
before running `backlot design sign [--skip]`. `i` opens the `tell hollywood` line, `n` opens the new-studio wizard (the bar
collapses first), `?` opens this help, `Esc` or the toggle collapses the bar. A prompt,
stage or finish action on a remote machine runs there through the shared RPC connection —
never SSH, never a local spawn; cards and `tell` run on the local core. The bar owns those
keys while it is expanded — an armed prefix disarms into it. A click on the pill expands it,
a click on a waiting row or talk line selects it (shifting focus to that zone) while clicking an
item's action chip (including `↵ read` on a talk line) runs that action, a click on a studio chip or
frame opens that studio and collapses the sheet, clicking `STUDIOS N` focuses `STUDIOS`, clicking
`[+ new]` opens the new-studio wizard, the wheel scrolls frames/strip sideways and waiting rows
vertically, and a click outside (including immediately below the `▀▀` bottom edge) collapses the
bar to the pill and hands the keys to what is under it — a typed letter lands in the pane.
Right-click on the title or a row opens the same context menu `m` opens.

`s` (or `Enter`) on a card, headline, or talk row opens it in the **Lot Reader** card: arrows / `j` / `k`, `Space`/`PageDown`/`Ctrl+D`, `b`/`PageUp`/`Ctrl+U`, `g`/`Home`, and `G`/`End` scroll (while plain `d` stays unbound), `←`/`→` (or `[`/`]` or clicking `◀`/`▶`) step sibling entries (or jump `## R<r>` / `### §R<r>.<W|D|P|U>` sections on `design/` Markdown docs), `y` copies raw unwrapped text via OSC 52 without answering prompts, `v` jumps to the studio's Task & Delivery Sheet, `o`/`p` focus or peek a live seat (when present), `t`/`i` (and `r` on an active `Design` doc) open inline compose sharing `hollywood.input` / studio draft and `↑`/`↓` history (`Esc` keeps draft), `?` opens key help, and `Esc`/`q` close (on read-only entries `Enter` closes too; on open cards `a`/`x` apply/cancel when unlocked and `a` confirms sign-off on waiting `Design` docs; `Enter` on cards is a no-op).

The input line (`Alt+Y` open, `i`, `Enter` on `›`, or a click) sends one line with
`backlot tell hollywood "<line>"` as the user — never written into Hollywood's terminal. While
composing, plain `↑`/`↓` step through `user` exchange history (`HollywoodSummary.exchange` plus
local sends, consecutive repeats collapsed), saving and restoring any unsent draft; `Alt+arrows`
remain studio/role navigation; `Enter` on non-empty text sends and stays in Compose (empty
rests); `Esc` always rests on `›` keeping the draft (never collapsing), and a second `Esc`
clears. While composing, the footer names `⏎ send   ↑↓ history   tab next zone   esc rows   alt+y close`,
fitting narrower widths down to `⏎ ↑↓ esc`.

Under the newest `user ›` line the band shows what Hollywood is doing with it, for as long as
that line has no `hollywood` reply after it: `… sending to hollywood` while the line is still in
the outbox, `⠶ hollywood: <doing>` while its terminal works (or `⠶ hollywood is working` when it
reports no detail), `○ hollywood is idle and has not answered · open ↵` once its screen reads idle
and the line still has nothing after it — a reply that never came, not silence — and
`⚠ hollywood asks: <prompt>` when a permission prompt stands on its
screen. A card Hollywood proposes (`card.proposed` by `hollywood`) ends the live row just like a
report: it draws as Hollywood's own line (`✦ proposed card c3 · acme-shop · pause t1`, the same
words the CARD row uses), under the line it answers, so a recast that came back as cards reads
as a reply where the person spoke. The prompt row takes the studio prompt row's keys: `y` answers `allow` and `d` `deny`
(`backlot approve --person hollywood --decision …` on Hollywood's own ledger), `p` peeks at its
screen read-only, and `Enter` focuses `bl-hollywood`. When Hollywood's runner has no approval key
(`can_answer_prompt` false — `opencode`, `cline`, `agy`) the row keeps its needs-you entry but the
detail reads `answer in its terminal (Enter)`, and `y`/`d` send nothing. The `allow` is the runner's **one-time**
key — cmd's `❯ 1. Yes` — never "don't ask again", which would allow every `backlot` command,
deletes included. Once a reply lands the row leaves; a `waiting` Hollywood after a reply shows
nothing, and the calm row and headline carry on as before. The stage sends the key only while
the prompt the decision was made on still shows. The prompt is found whichever channel names it —
the terminal's structured status (`kind: permission`, `state: blocked`), the hook signal, or a
picker on the screen — so a terminal resized narrow cannot hide the row. Its detail is unwrapped
from the screen's wrapping: the whole command for cmd, the question for the other runners, and
`permission prompt` when the screen cannot name it. The same width-independent picker detection
and detail reading raise a role's `permission prompt` attention.

The talk above the input keeps up to the **8 newest entries** when `HOLLYWOOD` is focused (folding
older turns onto the right end of the `── HOLLYWOOD ──` rule as `<k> earlier · ↵ read`), laid out
with a fixed speaker column (`✦ hollywood` / `› you` wide, `✦ hw` / `› you` narrow) and
right-aligned relative age (swapping for ` ↵ read ` when selected). Unfocused (when `WAITING` or
`STUDIOS` is focused), `HOLLYWOOD` collapses to the single newest talk line ending in `<age> · <N>
earlier` (or `<age>`) above compose. Enter on the fold badge or ` ↵ read ` chip opens the Reader,
where `←`/`→` still step through every earlier line. In focused `HOLLYWOOD`, any message wraps up to
two lines at the band's width using the same CJK-aware wrap the Reader body uses, clamping with
`…`; as height shrinks, older wraps and older lines give way back to `<k> earlier` without ever
pushing another zone's unfocused lines out. Selection, mouse hits and scrolling treat a wrapped
line as one row.


Everything the bar draws comes from the published `backlot` summary documents plus the
watcher's machine and stage state — never scraped terminal output — so the frames show even
while Hollywood's own agent is down (a `closed` or `lost` role state colours the title's
square, a broken studio's ledger error shows in its frame, and a stopped stage gets its own
band line). Without a table the frames still draw and the band shows the setup line instead
of the waiting rows — `⏎ hollywood setup` opens the in-TUI setup dialog. Everything it changes
goes through `backlot` commands run as the
user — `approve`, `stage start`, `card apply`, `card cancel`, `card show`,
`studio finish`, `tell hollywood`, `hollywood setup`, `design sign`, `design open`, and `report show` — off the event loop, failures surfacing as notices where
you are looking (on server clients, `design-sign`, `design-start-round`, and `tell` route over `plugin.backlot.req`; see [requests.md](requests.md) for `StudioSummary.design` fields and queue verbs, and [design.md](design.md#opening-design-discussion) for design lifecycle rules). The only client-local state is the mode, the selection, the scroll offsets
and the input line's text.

## Configuring Hollywood's agent

```toml
# ~/.config/backlot/config.toml
[hollywood]
runner = "claude"
model = "claude-sonnet-5-5"
effort = "high"
# key = "alt+y"               # optional
# default_cast = "/home/user/cast.toml"  # optional: the cast a studio draft starts from
```

`runner` and `model` are required, and the runner must be one backlot supports. `effort` follows
the cast file's rule: `cursor`, `devin` and `cmd` take none; every other runner needs one.

The section does not have to be written by hand:

```sh
backlot hollywood setup --runner claude --model claude-sonnet-5-5 --effort high
```

`hollywood setup` writes the `[hollywood]` table into the user config and keeps every other
line of the file as it was; it refuses — before touching the file — a runner that is not
supported or not installed (with the dry run's message), a bad model, an effort the runner's
rule does not take, a key value the layer could not bind, and a config file that does not
parse.

The TUI runs the same command for you before `[hollywood]` exists or while the stage is offline:
the expanded Call Sheet's last band line (`⏎ hollywood setup`), the expanded studio panel's
last row, and the settings overseer row all open the unified **switch/setup picker** — a runner
picked from `backlot runners --json` (installed only), that runner's model catalog with `auto`
first and free input kept (`e`), and its effort list where the runner has one. With a table
already written and Hollywood live, pressing `w` in the resting Call Sheet, clicking `[switch w]`
on a blocked/quota Hollywood row, opening `Switch agent…` (`w`) from the title/lot menu, or
selecting the settings overseer row opens the same picker prefilled from `[hollywood]` (and
prefers a healthy alternative runner when opened from a quota/error state).

When Hollywood is already live on a running stage, `backlot hollywood setup` refuses to edit
`config.toml` directly so it never triggers an unguarded break-before-make replacement — use
`backlot hollywood switch` (or `--force-offline` for offline config repair). Pickup: when
unconfigured or starting offline, a running **stage** reads `[hollywood]` on its next tick; a
running **client** reads `key`/`start` once at start. There is no file watcher.

## Switching Hollywood's agent

Switching a live Hollywood runs a **make-before-break handoff** (`backlot hollywood switch`) so
work and context survive quota exhaustion, auth failures, or model swaps with zero cooperation
from the running overseer:

```sh
backlot hollywood switch --runner codex --model gpt-6-sol --effort high
backlot hollywood switch --runner codex --model gpt-6-sol --effort high --timeout 90 --no-wait --json
backlot hollywood switch --status --json
backlot hollywood switch --cancel
```

`hollywood switch` is user-only: any attempt inside a role terminal (`BACKLOT_ROLE` or role
process-tree ancestor) refuses and points back to `backlot hollywood propose setup`. Switching
against a stopped stage also refuses (use `hollywood setup` offline).

Handoff protocol:

1. **Prepare & brief (`<state dir>/hollywood/handoffs/<id>.md`):** without touching `config.toml`
   or the active terminal, the stage validates the target identity and builds a redacted handoff
   brief containing an in-memory `status --all` ledger snapshot, open/applied cards, queued and
   unanswered user tells, `memory.md`, and a read-only conversation tail extracted for the prior
   runner (falling back cleanly to terminal scrollback plus ledger exchanges when no runner
   transcript file is available).
2. **Side-by-side candidate & probation (`Verifying` / `NeedsApproval`):** the stage launches a
   candidate terminal (`bl-hollywood-next` / `bl-hollywood-g<N>`, labelled
   `backlot.role=hollywood-handoff`) with an isolated signals file while keeping old Hollywood
   (`backlot.role=hollywood`) untouched. In the Call Sheet and Lot Pill (`switching…`), `p` peeks
   the candidate (`[peek next p]`), `x` cancels cleanly (`[cancel x]`), and `y`/`d` answers a
   candidate permission prompt (`[allow y]`). Delivery of queued user tells pauses behind the
   handoff until commit.
3. **Commit or rollback:** once the candidate proves live — either via its readiness `hollywood
   report` / `role.progress` carrying its handoff identity or via a clean `Working -> Waiting`
   completed turn with zero quota/auth/error banners — the stage atomically updates
   `config.toml`, promotes the candidate to `backlot.role=hollywood`, appends existing
   `role.closed` + `role.started` ledger events, stops the old agent process while retaining its
   read-only `hollywood-previous` screen for 10 minutes before sweep, and drains queued tells to
   the new terminal. Any pre-commit failure (exit, quota, auth, error, timeout, or cancel) removes
   the candidate cleanly and leaves `config.toml`, the old terminal, and all queued messages intact.

Active terminal resolution across the stage (`hlife`, `hwatch`, `hdeliver`) and client
(`hollywood_terminal`) reads `role.terminal` and `backlot.role=hollywood` rather than requiring a
hardcoded `"bl-hollywood"` pane name.

With a valid section the stage starts Hollywood in `<state dir>/hollywood/` (`backlot.role=hollywood`,
never `backlot.studio`) with `BACKLOT_ROLE=hollywood`, the state dir, and no `BACKLOT_STUDIO`. It
is adopted after a stage restart and resumed after a loss the way a producer is (three losses in
ten minutes is its `cannot resume` — that give-up holds while the same stage process lives;
restarting the stage is the way back, and the loss count starts over). A runner that mints its
session id after launch — `cmd`, `codex`, `devin`, `cursor`, `cline`, `agy` — is discovered the same
way a studio role's is, so a lost Hollywood resumes its own conversation; a resume never carries a
session id recorded under another runner, and a `closed` or a session-less `started` clears the old
one. Removing the section closes it. Its lifecycle lands in Hollywood's own ledger as `role.state`
events — the studios' four role events folded into one.

The stage owns the folder: `AGENTS.md` (the role prompt with its CLI reference, rewritten every
start and refreshed on adopt or stage boot whenever it differs), `memory.md` (made once, never
overwritten), `drafts/<studio>/` for studio-new work, plus `messages/`, `reports/` and
`roles/hollywood/`. Hollywood owes no steps and is never nudged.

The terminal starts at a readable grid — at least 100 columns and hc's `200×120` default
(`hlife::hsize`), so a person who peeks or focuses `bl-hollywood` sees its answers, not a
21-column strip. Peeking is strictly read-only: a peek attaches as `Role::Observe`, and an
observer never sends a size (only the focused pane and a visible size lease do). So no client
view can narrow `bl-hollywood` — only focusing it into a pane, the normal way, sizes it to that
pane.

### Its CLI reference, generated

Hollywood's `AGENTS.md` carries a **command reference generated from the CLI's own usage
strings** — every command it may run, one line each, with its flags — at a stable place, never
hand-copied. The stage regenerates it from `args_usage.rs` and `args_hwood.rs`
(`hwcli::generate`) into `prompts/hollywood-cli.md` and substitutes it for `{{CLI_REFERENCE}}` in
the prompt at every start. A usage string that gains or loses a flag fails the drift test until
the checked-in copy is refreshed (`BLESS=1 cargo test -p backlot-stage hwcli`). The reference
lists exactly the overseer set the launch pre-allows; the prompt says to use it and to run
`--help` only for a command it does not list, so Hollywood never probes its own CLI and never
blocks on a prompt for it (H2, P5).

## Cards

A card is Hollywood's only way to change anything. `backlot hollywood propose` records it in
Hollywood's own ledger; you apply or cancel it; only an applied card changes anything — and the
stage is what carries it out.

```sh
backlot hollywood propose pause --studio acme-shop --task t1 --why "actor is looping" --as hollywood
backlot hollywood propose abandon --studio acme-shop --task t1 --why "wrong approach" --as hollywood
backlot hollywood propose priority --studio acme-shop --task t1 --level high --why "blocks t2" --as hollywood
backlot hollywood propose close --studio acme-shop --person actor-t1-c1 --why "task done" --as hollywood
backlot hollywood propose recast --studio acme-shop --person director --runner cmd --model deepseek-v4.1-flash --why "director's runner keeps returning a quota error" --as hollywood
backlot hollywood propose recast --studio acme-shop --person actor-t1-c1 --runner codex --model gpt-5.3-codex --effort medium --why "the actor's runner keeps timing out" --as hollywood
backlot hollywood propose restart --studio acme-shop --person director --why "stuck at a prompt" --as hollywood
backlot hollywood propose cast --studio acme-shop --file drafts/acme-shop/cast.toml --why "cheaper actors for the second pass" --as hollywood
backlot hollywood propose reopen-studio --studio acme-shop --why "more work on the cart" --as hollywood
backlot hollywood propose setup --runner claude --model claude-opus-5-5 --effort xhigh --why "person asked to switch agent" --as hollywood
backlot hollywood propose repo-note --repo /home/user/shop --id build-cmd --text "Run scripts/farm-test.sh before review." --why "seen failing without farm script" --as hollywood
backlot hollywood withdraw c1 --reason "the quota came back" --as hollywood
backlot card list
backlot card apply c1
backlot card cancel c2 --reason "not now"
```

### Paths are resolved, and files snapshotted, when a card is proposed

A file a card names — `--cast`, `--brief`, or a `cast` card's `--file` — is resolved against
Hollywood's **working directory at propose time**, not against Hollywood's own folder at apply
time. A relative path becomes absolute, `..` is normalised away, and a file that is not there then
is refused at propose, never accepted and failed later at apply. `propose studio-new` runs the same
validation apply will run — `studio new --dry-run` — against the resolved files, so a card that
could not apply is refused before it exists.

The referenced cast and brief are **copied into the card's own directory** (`cards/c<n>/` under
Hollywood's ledger) at propose, with their SHA-256 hashes recorded on the card. Apply reads the
copy — what the person reviewed is what executes. If the source file has changed since the
proposal, the card goes **stale** with `the cast file changed after this card was proposed`, like
the other base checks. `card list` and `card show` print a one-line roles summary for a card that
carries a cast (`producer=claude/opus-5-5 director=claude/opus-5-5 writer=cmd/deepseek…`), read
from the snapshot.

### Withdrawing, and superseding

`backlot hollywood withdraw <card> --reason <text>` takes back a card Hollywood proposed that is
still **open**; the ledger records who withdrew it and why (`card.withdrawn`), and the TUI shows it
as `withdrawn`. Hollywood may withdraw only its own card — never the user's, and never the stage's
outward card — and not one already applied, cancelled or stale.

A **newer card replaces an older conflicting one**. When a new card targets the same single fact an
open card already targets — a role's identity (`recast`), a person's terminal (`restart`), a
studio's cast (`cast`), a studio's finished state (`reopen-studio`), a studio name (`studio-new`),
a repo note `(repo, entry_id)` (`repo-note`),
or Hollywood's own identity (`setup`) — the older card is recorded `superseded by c<n>` in the same
append. A batch apply can therefore never run both. Cards with a sequence rather than one target
(`pause`/`resume` on a task, `priority`, `close`/`reopen` on a person) never supersede.


Each card carries the exact action, a one-line why, and its **base** — the fact about the studio
that must still be true when you apply it. A `pause` card bases on the task running; a `studio-new`
card bases on the name being free; `close`/`reopen` base on the person's lifecycle; a `priority`
card records the level it replaces; a `recast` card bases on the fixed role's current identity and
on the studio not being finished. `propose` refuses a card born stale — proposing `pause` on a
task that is not running fails before the card exists, and a `recast` runs the same validation as
`backlot recast` — and `card apply` re-checks: a base that moved since proposal records
`card.stale` with the reason instead of `card.applied`.

The stage carries an applied card out once: it checks the base **again** (a base that moved since
the apply records `card.stale` and nothing else happens), then records the action in the studio's
ledger exactly as your own command would — `by: user`, through the same validation, a refusal
there landing `card.failed` with the reason. Success is `card.done`. Never twice: before touching
the studio the stage asks whether the card's effect is already there — the `card c<n>` marker on
a `task.state`, the priority it wanted, the `purpose: "brief"` message a `studio-new` leaves — and
an effect already in the ledger records `card.done` without re-appending, so a restart between the
action and its record repeats nothing. Cards execute whether or not `[hollywood]` is configured:
the config gates the agent, never the gate.

Card actions (D18): `studio-new`, `close`, `reopen`, `reopen-studio`, `priority`, `pause`, `resume`,
`abandon`, `recast`, `restart`, `cast`, `setup`, `outward`, `repo-note`. Nothing else — in particular, no card answers a
permission prompt; those stay yours, answered in the studio.

A `recast` card swaps the agent that plays a fixed role — `producer`, `director` or `writer` —
or an actor, by its key `actor-<task>-c<n>`. Its text reads `recast director → cmd/deepseek-v4.1-flash`
(`<runner>/<model>[/<effort>]`), and `card show` prints the old and the new identity. Applied, the
stage closes the old terminal (`role.closed`, reason `recast` — a deliberate stop, never a loss)
and starts a fresh conversation under the new identity. It never resumes the old session id: that
conversation belongs to the runner that left. The ledger and the messages already queued for the
role stay; an actor keeps its key and its work area, and the task's deliveries, rework rounds and
budget are untouched. A recast while the old role has an open permission prompt still closes it.
Only you may recast, and only while the studio is not finished.

A `restart` card reloads a live or stuck person in place — the same command `backlot restart` runs.
It bases on `person_restartable`: the person is live or lost, never closed (reopen is that) and the
studio is not finished. Applied, the stage stops the terminal (`role.closed`, reason `restarted`)
and launches it again under the same identity, resuming the recorded session or `--fresh` starting
a new one. The restart's `role.restart` carries the card's `card c<n>:` reason, so a restart between
the append and `card.done` repeats nothing.

A `cast` card changes the studio's cast after it started — the settings change from
[`backlot studio cast set`](guide.md#changing-a-studio-after-it-starts). `--file` names a cast
draft, read with `studio new`'s parser and checked with the same runnable rules. It bases on
`cast_seq`: the seq of the `studio.cast` event it edits (`0` when the cast has never changed), so a
change in between makes it stale. `card show` prints the dry-run diff the file would make. Applied,
the stage records the recasts and one `studio.cast` `by: user`, idempotent like the other cards
(the newest `studio.cast` carries the card's `c<n>` marker).

A `reopen-studio` card brings a finished studio back. Reopening is the person's own command
(`backlot studio reopen`); this card is how Hollywood asks for it. It bases on `studio_finished`:
the studio is finished now, so a studio that is already running refuses the card at propose. Applied,
the stage records `studio.reopened` `by: user` with the card's marker (a restart between the append
and `card.done` repeats nothing), and recasts, cast changes and new tasks work again.

A `setup` card changes Hollywood's own identity (`Hollywood: <old> → <new>`) against a
`hollywood_identity` base. `propose setup` validates the runner, model and effort rule up front
just like `backlot hollywood setup`. Applied (`by: user`), the stage runs `hollywood setup`'s
config write (preserving `start` and every other key in `[hollywood]`), records `card.done`,
and restarts Hollywood fresh under the new identity with a takeover first prompt naming what changed.

Actionable card failures (`card.failed`, `card.stale`, `card.cancelled`) reach Hollywood's inbox as
notices; `card.applied` emits no wakeup notice, and `card.done` stays passive context that rides
with the next delivery.

A `repo-note` card records a durable, cross-studio constraint or convention for a canonical repository path (`--id` matching `[a-z0-9_-]{1,40}`, body `<= 1024` bytes, statically checked against secret/DSN patterns, `#`-prefixed heading lines, and `<!--` comment markers at propose time). It bases on `repo_note_entry { repo, entry_id, prev_sha256 }` per `(repo, entry_id)` so unrelated notes applied in one batch never make each other stale, while a second card targeting the same `(repo, entry_id)` supersedes open ones. Both `backlot hollywood propose repo-note` and studio-side `backlot repo-note propose` (bridged once from a `purpose = "repo_note"` studio message by the stage) produce the same card. On apply, the stage atomically updates `<state_dir>/repo-notes/<sha256(canonical_repo)[..16]>.md` at mode `0600` using a `<!-- card c<seq> -->` idempotency marker — never writing inside the target repository. When future `director` or `writer` sessions start on the same repo and approved notes exist, the stage copies them to a `0400` snapshot at `<studio_dir>/repo-notes.md` (rewritten on every start/resume, so editing the snapshot changes nothing) and renders `{repo_notes}` in their prompt pointing at that file — working uniformly across every runner without widening any runner allowlist (`backlot repo-note show|remove` let the user inspect or delete approved entries).

## Creating a studio in plain words

Tell Hollywood what you want — the goal, the repository, who should work on it — and it writes a
draft in its own folder, checks it with `studio new --dry-run`, and proposes a `studio-new` card
only when the draft passes every check `studio new` runs. Applying the card creates the studio;
the brief becomes the producer's first message. What you did not say is not guessed: it comes
from `[hollywood] default_cast`, else the last studio's cast, else Hollywood asks.

```sh
backlot studio new --title Shop --repo /home/user/acme-shop --cast /home/user/cast.toml --dry-run --studio acme-shop
backlot hollywood propose studio-new --studio new-shop --title "New shop" --repo /home/user/shop --cast /home/user/hollywood/drafts/new-shop/cast.toml --brief /home/user/hollywood/drafts/new-shop/brief.md --why "user asked" --as hollywood
```

`--dry-run` runs every validation `studio new` runs — the name, the reserved name `hollywood`,
whether the name is taken, whether `--repo` is a git repository, the cast file, the declared
checks, every runner supported *and installed*, and the caps seating the fixed roles — and writes
nothing: no ledger event, no directory. One refusal per kind, each saying what to fix.

The dry run runs again at apply time, inside the card's execution. `studio.created` lands
`by: user` with the draft's cast file copied into the studio; the brief is queued as a
`purpose: "brief"` message to the producer — a purpose-bearing message is the stage's or the
producer's word, so the stage records that one. A studio the card already made (its brief is the
marker) records `card.done` without repeating; one made any other way is a moved base.

## Talking to Hollywood, and what it may say

`backlot tell hollywood` records your line in Hollywood's ledger, and the stage delivers it — the
same guarded send the studios get: only when `bl-hollywood` is live, waiting, past its
up-time window and showing a stable screen, never into a dialog. A `message.delivered` records
the send.

```sh
backlot tell hollywood "watch acme-shop for stalls"
```

The stage runs a **two-lane inbox** for Hollywood:

- **Person lane first (`from == "user"`).** Pending `tell hollywood` messages deliver oldest-to-newest
  before stage notices — merged onto one line joined with ` · ` when `≤ 2 KiB`, or written to
  `messages/user-<top>.md` with a 120-character latest preview when longer. Waiting stage notices
  append a footer (`(+N stage notices waiting: <labels>)`) without blocking your message.
- **Stage-notice lane (5 s defer).** Only when the Person lane is empty and the newest actionable
  notice is at least 5 s old does the stage deliver notices, giving transient prompts time to clear.
  A single notice `≤ 400 B` sends inline; two or more (or longer) send as `messages/digest-<top>.md`.
  Notices rank `stalled`, `failed`, `card`, `prompt`, `attention`, `finished`, `recovered`.
- **Notice hygiene & passive context.** Closed or `info` attentions (`session id not found`) and
  `card.applied` emit no wakeups; `stuck` attentions (`stalled`, `rate limited`, `connection error`,
  `question`, director-handled actor prompts and `agent error`) wait until open past
  `default_stall_limit_s` (10 minutes) before waking Hollywood (naming how long an overdue actor
  prompt waited and why), while `needs you` attentions and labelled notices (`finished`, `failed`,
  `card`, `prompt`, `recovered`) fire at once. Queued notices coalesce to `≤ 1` per `(studio, role, label)` and
  `≤ 1` `recovered` (dropped on a fresh start/close, on outcome, or when a studio finishes).
  `card.done` and accepted reviews never wake an idle Hollywood on their own — they ride along once
  as `(context: …)` on the next delivery.
- **`--answers <seq>[,<seq>]`.** Pass `--answers` on `hollywood propose` or `hollywood report` when
  acting on queued tells you saw early; the stage appends `message.seen` (`by_seq`) so those tells
  are never pasted into the terminal again (already-delivered or seen seqs are skipped silently).

### What Hollywood is fed: facts with their age and class

Hollywood is fed facts, not jargon. Every attention it can read — in `backlot status`, in a
notice's body, in the status commands it runs — carries the same shaping, from one function
(`backlot_model::attention::AttentionClass`):

- **its class:** `needs you`, `stuck` or `info`;
- **its age:** `raised 2h ago`;
- **for a screen snapshot,** when it was taken: `screen at 14:02, may be stale`.

Each role line also carries its **current live work state** — `working`, `waiting` or
`blocked` — from the stage's last watch sample, read out of `<state dir>/live.json` so a role
terminal's `backlot status` sees the same picture the client draws. The stage writes that file on
a change and a heartbeat (at most every 5 s), and a reader drops a file older than 15 s: after the
stage stops, a role line carries no work word and `backlot status` says once, under `roles:`,
`stage not running — live state unknown` — the file's age is a liveness signal, never a stale
`· working`. That is what the round-5 miss looked like: Hollywood read an attention's saved screen
— agy's sign-in splash, captured at launch — as the current state, and reported a healthy producer
as "not signed in". The prompt's rule, the fact shaping above and the age check are what stop it:
**decide from the role's current state, never from an old snapshot; `info` items are bookkeeping
and are never reported as problems.**

### What Hollywood may run

Hollywood's own reports come back through `backlot hollywood report` — the newest headline is
what the collapsed strip and the expanded panel show, read from the ledger into the summary
document's `hollywood` object (D21). It may also talk *to* a studio's producer on your behalf — its one write into a studio:
`tell producer` sent `--as hollywood` records the studio's `message.queued` as the delivery
record, and mirrors `message.sent` into Hollywood's own ledger.

```sh
backlot hollywood report "two studios idle, one card open" --answers 42 --as hollywood
backlot tell producer --file /home/user/note.md --studio acme-shop --as hollywood
```

## What Hollywood may and may not do

Its role rules, enforced at append:

- **Reads everything**: `status`, `records` and `studio new --dry-run` — with `--studio <name>`
  or `--all` to pick (`status --all` shows every studio then Hollywood's cards and headline).
- **Writes only its own ledger**: card proposals, reports, and the message mirror. Inside a
  studio's ledger the only event it may record is a plain `tell` *to the producer* — never to
  directors, writers or actors, never an `ask` or a report, and never any other studio event
  (R26).
- **Cannot act for you**: `card apply` and `card cancel` refuse whenever `BACKLOT_ROLE` is set —
  a role terminal is never the user. And the stage is never Hollywood's either.
- **`hollywood` is a reserved name**: no studio can take it; its ledger is not a studio.

And the honesty note (D17a): the card gate is a **working rule, not a security boundary**. Identity
in backlot comes from the environment (`BACKLOT_ROLE`, `--as`), so the CLI cannot *prove* an apply
was yours. `--as user` in a role terminal is refused — it disagrees with the `BACKLOT_ROLE` pin —
but a role terminal that unsets `BACKLOT_ROLE` looks like yours, and that is what could run
`card apply`. What holds in practice is the pair of rules above plus the runner's own permission
prompt: Hollywood's launch pre-allows exactly `backlot status`, `backlot records`,
`backlot tell <studio> producer`, `backlot progress`, `backlot hollywood propose`,
`backlot hollywood report`, `backlot hollywood withdraw`, `backlot studio new --dry-run`,
`backlot studio cast set --dry-run`, and on `claude`/`cmd` the read-only `backlot --help` /
`backlot help`, so anything else it types stops at a prompt you see. That prompt is yours: the
stage sends a key only for a decision you recorded — `y`/`d` on the bar's Hollywood prompt row —
and only while the prompt that decision answered still shows.

The pre-allow list is per-runner: `claude`, `cursor`, `cmd` and `cline` wire it into the launch
setup.
`codex` and `opencode` take no command list — there the runner's own permission mode
(`on-request`, the env defaults) is the gate, and Hollywood's role rules still refuse at
the ledger. `devin` takes no argv list either, and — since Hollywood gets none of the shared
read-only set — `smart` alone governs it. `backlot guide` is on every list, the compiled-in docs.

## What is not here yet

- **H5**: a real agent under an approved run — everything above is runner-agnostic and fake-tested.
- **Mobile (`wherdr`) opening design UI**: `backlot/1` (`StudioSummary.design`), `design/*` report reads, and the `plugin.backlot.req` actions (`design-sign`, `design-start-round`, `tell`) are implemented on the hub, while `wherdr` client cards and buttons are not built yet.

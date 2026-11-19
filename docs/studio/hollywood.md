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
- **Collapsed** is the **lot pill** in the chrome row — a reserved, clickable segment at the
  right end of the tab bar, or of the one-row studio header on a studio workspace. It counts
  the urgent items (user prompts you own, stopped stages, ledger errors), the open cards and
  studios ready to finish, and the studios (`✦`); urgent counts turn it red, and narrow widths
  shrink it through shorter texts. A click or the toggle key expands the band.
- **Expanded** is the **Call Sheet** — the film-strip band dropped from the top over the
  whole width, at most half the height (sixteen rows on a mobile width): `▄▄`/`▀▀` sprocket
  rows, the `✦  H O L L Y W O O D` title with the agent's square, role state and runner /
  model / effort plus the right-hand studio count and `k waiting on you` (red while a prompt
  or a down stage waits, peach otherwise), then one seven-row frame per studio — the
  machine-qualified id in the top edge (accent on `selection_bg` when selected), its own
  machine's `■ live` / `◌ stage down` / `× offline` and a ` !n ` chip, `r<ver> · <done>/<total>`
  task counts, the role on set, what it is doing (a broken ledger's error in red), and one
  `◆` per cast member coloured by its state. The last frame is always the dashed
  **new-studio** frame; frames that do not fit scroll sideways behind `‹`/`›`, a narrow screen
  puts two per row. Under them the band lists the **waiting items** — everything that waits on
  the user across every machine, sorted by urgency: user-owned prompts first (a
  director-owned prompt never lists), then stopped stages and offline machines that have live
  studios, then the local core's open cards, then studios ready to finish. Each row carries
  its action chips and a detail line; when nothing waits, a calm `✓` row says so. The
  `tell hollywood` line ends the band — or, without a `[hollywood]` table, the line that names
  `backlot hollywood setup`. Waiting items and the `tell` line are never dropped: the height
  ladder drops the blanks first, then the unselected details, then the 7-row frames fold into
  a one-row chip strip, then the headline, then the hints and sprockets — and when the items
  still outgrow the room the window scrolls and a `i/N` marker says how much is showing.

The start policy is **`collapsed`** by default (`[hollywood] start` — `collapsed`,
`when_waiting`, or `expanded`), applied once the first settled `backlot` read lands.
`when_waiting` opens expanded only while the urgent count is above zero — and again on the edge
up — and a studio that arrives later never pulls the bar open. Neither the card nor the bar
draws before that first read settles (two seconds at most), so a client with studios never
flashes the welcome card.

The key is **`alt+y`**, a direct key with no prefix, plus the `Ctrl+B i` chord tail. It switches
collapsed and expanded and shows the layer when hidden; `x` in the expanded bar hides it; `Esc`
collapses it. `[keys] call_sheet` rebinds the direct key and `[keys] prefix_call_sheet` the
chord tail — an empty string unbinds that slot; `[hollywood] key` fills whichever slot `[keys]`
leaves unset (a direct binding like `alt+y`, or `prefix+<key>` for a chord). It is
bound in studio mode with or without a `[hollywood]` table; under `--plain` it is not bound, so
Alt+Y reaches the focused program as always. A key value that does not parse, is a bare printable
key, or collides with one of the four studio keys or the panel chord falls back to `alt+y` with a
notice that names the file and never echoes the value. Known costs, said plainly:

1. While bound, **Alt+Y no longer reaches the program in the pane**. Readline and Emacs use `M-y`
   for yank-pop; if you use it, rebind (`key = "prefix+h"`) or unset the layer.
2. On a terminal **without the kitty keyboard protocol**, Esc followed by `y` inside the escape
   window (150 ms while the mouse is captured) reads as Alt+Y. Rare in practice; the same rebind
   fixes it.
3. On **macOS**, the terminal must send Option as Meta.

The studio panel keeps `prefix+y`.

Inside the expanded bar: `←` `→` (or `h` `l`) move between the frames, the new-studio frame
included; `↓` `↑` (or `j` `k`) leave the frames for the waiting rows and come back;
`PageUp` / `PageDown` scroll the waiting list; `1`–`9` jump to a frame and `0` to the
new-studio frame; `Enter` activates the selection. Each waiting row has its own keys: on a
**prompt** row `y` answers `allow` and `d` answers `deny` (`backlot approve --person <role>
--decision <d>` on the prompt's own machine), `p` peeks at the role's live screen read-only;
on a **stage** row `r` runs `backlot stage start` on that machine, and an offline machine's
`Enter` reports the watcher's last read; on a **card** row `a` applies, `x` cancels and `s`
shows the card (`backlot card show` in a read-only view); on a **ready** row `f` asks
`finish studio? y/n` inline, and `y` runs `backlot studio finish` on the studio's own
machine. `i` opens the `tell hollywood` line, `n` opens the new-studio wizard (the bar
collapses first), `?` opens this help, `Esc` or the toggle collapses the bar. A prompt,
stage or finish action on a remote machine runs there through the shared RPC connection —
never SSH, never a local spawn; cards and `tell` run on the local core. The bar owns those
keys while it is expanded — an armed prefix disarms into it. A click on the pill expands it,
a click on a row selects it and acts like `Enter`, a click on a chip runs that chip's action,
the wheel scrolls the frames sideways and the waiting rows vertically, and a click outside
collapses the bar to the pill and hands the keys to what is under it — a typed letter lands in
the pane. Right-click on the title or a row opens the same context menu `m` opens.

`s` on any card opens `backlot card show <id>` in the read-only view. The view owns its keys
until Esc, Enter, `x` or `q` closes it — arrows / `j` `k` scroll.

The input line (`i` or a click) is one line sent with `backlot tell hollywood "<line>"` as the
user — never written into Hollywood's terminal. Empty sends nothing; `Esc` leaves the line;
Enter sends and leaves it.

Everything the bar draws comes from the published `backlot` summary documents plus the
watcher's machine and stage state — never scraped terminal output — so the frames show even
while Hollywood's own agent is down (a `closed` or `lost` role state colours the title's
square, a broken studio's ledger error shows in its frame, and a stopped stage gets its own
band line). Without a table the frames still draw and the band shows the setup line instead
of the waiting rows. Everything it changes goes through `backlot` commands run as the
user — `approve`, `stage start`, `card apply`, `card cancel`, `card show`,
`studio finish`, `tell hollywood` — off the event loop, failures surfacing as notices where
you are looking. The only client-local state is the mode, the selection, the scroll offsets
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
parse. While no section exists, the expanded studio panel's last row names this command.

Pickup: a running **stage** re-reads the table on its next tick and starts `bl-hollywood`; a
running **client** reads the config once, at start — a layer appears or its key changes at the
next client start. There is no file watcher.

With a valid section the stage runs **one terminal named `bl-hollywood`** — labelled
`backlot.role=hollywood` and never `backlot.studio` — in `<state dir>/hollywood/` with
`BACKLOT_ROLE=hollywood`, the state dir, and no `BACKLOT_STUDIO`. It is started, adopted after a
stage restart, and resumed after a loss the way a producer is (three losses in ten minutes is its
`cannot resume` — that give-up holds while the same stage process lives; restarting the stage is
the way back, and the loss count starts over). Changing the runner starts a new conversation;
removing the section closes it.
Its lifecycle lands in Hollywood's own ledger as `role.state` events — the studios' four role
events folded into one.

The stage owns the folder: `AGENTS.md` (the role prompt, rewritten every start), `memory.md`
(made once, never overwritten), `drafts/<studio>/` for studio-new work, plus `messages/`,
`reports/` and `roles/hollywood/`. Hollywood owes no steps and is never nudged.

## Cards

A card is Hollywood's only way to change anything. `backlot hollywood propose` records it in
Hollywood's own ledger; you apply or cancel it; only an applied card changes anything — and the
stage is what carries it out.

```sh
backlot hollywood propose pause --studio acme-shop --task t1 --why "actor is looping" --as hollywood
backlot hollywood propose abandon --studio acme-shop --task t1 --why "wrong approach" --as hollywood
backlot hollywood propose priority --studio acme-shop --task t1 --level high --why "blocks t2" --as hollywood
backlot hollywood propose close --studio acme-shop --person actor-t1-c1 --why "task done" --as hollywood
backlot card list
backlot card apply c1
backlot card cancel c2 --reason "not now"
```

Each card carries the exact action, a one-line why, and its **base** — the fact about the studio
that must still be true when you apply it. A `pause` card bases on the task running; a `studio-new`
card bases on the name being free; `close`/`reopen` base on the person's lifecycle; a `priority`
card records the level it replaces. `propose` refuses a card born stale — proposing `pause` on a
task that is not running fails before the card exists — and `card apply` re-checks: a base that
moved since proposal records `card.stale` with the reason instead of `card.applied`.

The stage carries an applied card out once: it checks the base **again** (a base that moved since
the apply records `card.stale` and nothing else happens), then records the action in the studio's
ledger exactly as your own command would — `by: user`, through the same validation, a refusal
there landing `card.failed` with the reason. Success is `card.done`. Never twice: before touching
the studio the stage asks whether the card's effect is already there — the `card c<n>` marker on
a `task.state`, the priority it wanted, the `purpose: "brief"` message a `studio-new` leaves — and
an effect already in the ledger records `card.done` without re-appending, so a restart between the
action and its record repeats nothing. Cards execute whether or not `[hollywood]` is configured:
the config gates the agent, never the gate.

Card actions (D18): `studio-new`, `close`, `reopen`, `priority`, `pause`, `resume`, `abandon`.
Nothing else — in particular, no card answers a permission prompt; those stay yours, answered in
the studio.

Every card outcome reaches Hollywood's inbox as a notice: applied, cancelled, done, failed, stale.

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

The stage also tells Hollywood, each once and naming the studio: an attention opened, a prompt
only you may answer, a studio finished or failed, a delivery accepted, a stall it could not
route. Notices follow the studios' own rules — the body lives under `messages/`, several
undelivered items go as one digest, and a queued notice whose source moved on is dropped, not
sent (`message.dropped`). Hollywood's own words — a recovered terminal, each card's outcome —
join the same inbox.

Hollywood's own reports come back through `backlot hollywood report` — the newest headline is
what the collapsed strip and the expanded panel show, read from the ledger into the summary
document's `hollywood` object (D21). It may also talk *to* a studio's producer on your behalf — its one write into a studio:
`tell producer` sent `--as hollywood` records the studio's `message.queued` as the delivery
record, and mirrors `message.sent` into Hollywood's own ledger.

```sh
backlot hollywood report "two studios idle, one card open" --as hollywood
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
`backlot tell <studio> producer`, `backlot hollywood propose`, `backlot hollywood report` and
`backlot studio new --dry-run`, so anything else it types stops at a prompt you see. That prompt
is yours — the stage never answers a prompt in Hollywood's terminal.

The pre-allow list is per-runner: `claude`, `cursor` and `cmd` wire it into the launch argv.
`devin`, `codex` and `opencode` take no command list — there the runner's own permission mode
(`smart`, `on-request`, the env defaults) is the gate, and Hollywood's role rules still refuse at
the ledger.

## What is not here yet

- **H5**: a real agent under an approved run — everything above is runner-agnostic and fake-tested.

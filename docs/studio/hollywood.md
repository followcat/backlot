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
  right end of the tab bar, or of the one-row studio header on a studio workspace. Its urgent
  count is the length of the one needs-you list — user prompts, roles with a needs-you attention
  or an unanswered question, blocked Hollywood, studios with no requirement, stopped stages,
  open cards and studios ready to finish — so it can never disagree with the sheet or the calm
  row; it also counts the studios (`✦`) and, while collapsed, new Hollywood replies. Urgent
  counts turn it red, and narrow widths shrink it through shorter texts. A click or the toggle
  key expands the band.
- **Expanded** is the **Call Sheet** — the film-strip band dropped from the top over the
  whole width, at most half the height (sixteen rows on a mobile width): `▄▄`/`▀▀` sprocket
  rows, the `✦  H O L L Y W O O D` title with the agent's square, role state and runner /
  model / effort plus the right-hand studio count and `k waiting on you` (red while a prompt
  or a down stage waits, peach otherwise), then one seven-row frame per studio — the
  machine-qualified id in the top edge (accent on `selection_bg` when selected), its own
  machine's `■ live` / `◌ stage stopped` / `× offline` and a ` !n ` chip, `r<ver> · <done>/<total>`
  task counts, the role on set, what it is doing (a broken ledger's error in red), and one
  `◆` per cast member coloured by its state. A frame whose studio has a plain next step carries
  `· next: tell the producer what to build`, `· next: director reviewing` or `· next: finish (f)`
  on its blank row. The last frame is always the dashed
  **new-studio** frame; frames that do not fit scroll sideways behind `‹`/`›`, a narrow screen
  puts two per row. Under them the band lists the **waiting items** — the one needs-you list,
  everything that waits on the user across every machine, sorted by urgency: user-owned prompts
  first (a director-owned prompt never lists), then roles with a needs-you attention or an
  unanswered question, Hollywood itself — `HOLLYWOOD asks: <prompt>`, which takes the same
  `y` allow once / `d` deny / `p` peek / Enter as its live row, or `HOLLYWOOD blocked` without a
  prompt — studios with no requirement, stopped stages and offline machines that have live
  studios, then the local core's open cards, then studios ready to finish. Each row carries its
  action chips and a detail line; only when the list is empty does a calm `✓` row say
  `nothing waiting on you`. The
  `tell hollywood` line ends the band — or, without a `[hollywood]` table, the `⏎ hollywood setup`
  line that opens the setup dialog. Waiting items and the `tell` line are never dropped: the height
  ladder drops the blanks first, then the unselected details, then the 7-row frames fold into
  a one-row chip strip, then the headline, then the hints and sprockets — and when the items
  still outgrow the room the window scrolls and a `i/N` marker says how much is showing.

The start policy is **`collapsed`** by default (`[hollywood] start` — `collapsed`,
`when_waiting`, or `expanded`), applied once the first settled `backlot` read lands.
`when_waiting` opens expanded only while the urgent count is above zero — and again on the edge
up — and a studio that arrives later never pulls the bar open. Neither the card nor the bar
draws before that first read settles (two seconds at most), so a client with studios never
flashes the welcome card.

The key is **`alt+y`**, a direct key with no prefix, plus the `Ctrl+B i` chord tail. From Hidden
or Collapsed it expands straight into Compose (`›`) when `[hollywood]` is set (waiting items stay
visible above; unconfigured keeps wait/frame landing), and from Expanded it collapses keeping any
draft; `Esc` in Compose rests on `›` keeping the draft, and a second `Esc` in Resting collapses
it. Toast jumps (`Ctrl+B o` or click) still land on their named target in Resting. `[keys]
call_sheet` rebinds the direct key and `[keys] prefix_call_sheet` the
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

The studio panel keeps `prefix+y` to toggle, and **`alt+l`** (`[keys] focus_lot`, `""` unbinds)
focuses the right-hand lot (`lot · N`) on its last selectable row — opening it if hidden or
collapsed without starting compose or toggling closed (shown beside `alt+y / ctrl+b i` in the lot
header, `?`, and the launcher; falls through under `--plain`).

Inside the expanded bar: `Tab` / `Shift+Tab` cycles the three zones — `Compose (›)` →
`Waiting/Exchange` (first waiting item, or newest exchange row) → `Studios` (upper frames) —
skipping empty zones (`Shift+Tab` reverses); in Resting, `←` `→` (or `h` `l`) move between the
frames (new-studio included) or action chips, `↓` `↑` (or `j` `k`) walk within the active section,
`PageUp` / `PageDown` scroll the waiting list, `1`–`9` jump to a frame and `0` to the
new-studio frame, and `Enter` activates the selection. Each waiting row has its own keys: on a
**prompt** row `y` answers `allow` and `d` answers `deny` (`backlot approve --person <role>
--decision <d>` on the prompt's own machine), `p` peeks at the role's live screen read-only;
on a **stage** row `s` runs `backlot stage start` on that machine, and an offline machine's
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

The input line (`Alt+Y` open, `i`, `Enter` on `›`, or a click) sends one line with
`backlot tell hollywood "<line>"` as the user — never written into Hollywood's terminal. While
composing, plain `↑`/`↓` step through `user` exchange history (`HollywoodSummary.exchange` plus
local sends, consecutive repeats collapsed), saving and restoring any unsent draft; `Alt+arrows`
remain studio/role navigation; `Enter` on non-empty text sends and stays in Compose (empty
rests); `Esc` always rests on `›` keeping the draft (never collapsing), and a second `Esc`
collapses. While composing, the footer names `⏎ send   ↑↓ history   tab waiting   esc rows   alt+y close`
(or `tab studios` when nothing waits), fitting narrower widths down to `⏎ ↑↓ esc`.

Under the newest `user ›` line the band shows what Hollywood is doing with it, for as long as
that line has no `hollywood` reply after it: `… sending to hollywood` while the line is still in
the outbox, `⠶ hollywood: <doing>` while its terminal works (or `⠶ hollywood is working` when it
reports no detail), and `⚠ hollywood asks: <prompt>` when a permission prompt stands on its
screen. The prompt row takes the studio prompt row's keys: `y` answers `allow` and `d` `deny`
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

Everything the bar draws comes from the published `backlot` summary documents plus the
watcher's machine and stage state — never scraped terminal output — so the frames show even
while Hollywood's own agent is down (a `closed` or `lost` role state colours the title's
square, a broken studio's ledger error shows in its frame, and a stopped stage gets its own
band line). Without a table the frames still draw and the band shows the setup line instead
of the waiting rows — `⏎ hollywood setup` opens the in-TUI setup dialog. Everything it changes
goes through `backlot` commands run as the
user — `approve`, `stage start`, `card apply`, `card cancel`, `card show`,
`studio finish`, `tell hollywood`, `hollywood setup` — off the event loop, failures surfacing as notices where
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
parse.

The TUI runs the same command for you: while no section exists, the expanded Call Sheet's last
band line (`⏎ hollywood setup`), the expanded studio panel's last row and the settings overseer
row all open the **setup dialog** — a runner picked from `backlot runners --json` (installed
only), that runner's model catalog with `auto` first and free input kept (`e`), and its effort
list where the runner has one. With a table already written, `m` on the Call Sheet title row
(`Change Hollywood's agent…` — the same runner/model/effort picker `Recast…` uses, prefilled
from `[hollywood]`), `c` on the Call Sheet, or the same settings row opens setup — its
`key`/`default_cast` (plus `start` and other keys in `[hollywood]`) ride along — and changing
the identity starts a fresh conversation while `memory.md` stays.
The write and every refusal stay `hsetup`'s own; a success flips the bar to its agent form on
the next tick.

Pickup: a running **stage** re-reads the table on its next tick and starts `bl-hollywood`; a
running **client** reads the config once, at start — a layer appears or its key changes at the
next client start. There is no file watcher.

With a valid section the stage runs **one terminal named `bl-hollywood`** — labelled
`backlot.role=hollywood` and never `backlot.studio` — in `<state dir>/hollywood/` with
`BACKLOT_ROLE=hollywood`, the state dir, and no `BACKLOT_STUDIO`. It is started, adopted after a
stage restart, and resumed after a loss the way a producer is (three losses in ten minutes is its
`cannot resume` — that give-up holds while the same stage process lives; restarting the stage is
the way back, and the loss count starts over). A runner that mints its session id after launch —
`cmd`, `codex`, `devin`, `cursor`, `cline`, `agy` — is discovered the same way a studio role's
is, so a lost Hollywood resumes its own conversation; a resume never carries a session id
recorded under another runner, and a `closed` or a session-less `started` clears the old one.
Changing the runner starts a new conversation; removing the section closes it.
Its lifecycle lands in Hollywood's own ledger as `role.state` events — the studios' four role
events folded into one.

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
backlot hollywood propose setup --runner claude --model claude-opus-5-5 --effort xhigh --why "person asked to switch agent" --as hollywood
backlot card list
backlot card apply c1
backlot card cancel c2 --reason "not now"
```

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

Card actions (D18): `studio-new`, `close`, `reopen`, `priority`, `pause`, `resume`, `abandon`,
`recast`, `setup`. Nothing else — in particular, no card answers a permission prompt; those stay yours,
answered in the studio.

A `recast` card swaps the agent that plays a fixed role: `producer`, `director` or `writer`. Its
text reads `recast director → cmd/deepseek-v4.1-flash` (`<runner>/<model>[/<effort>]`), and
`card show` prints the old and the new identity. Applied, the stage closes the old terminal
(`role.closed`, reason `recast` — a deliberate stop, never a loss) and starts a fresh conversation
under the new identity. It never resumes the old session id: that conversation belongs to the
runner that left. The ledger and the messages already queued for the role stay; the new terminal
takes over the role's work. A recast while the old role has an open permission prompt still closes
it. Only you may recast, and only while the studio is not finished.

A `setup` card changes Hollywood's own identity (`Hollywood: <old> → <new>`) against a
`hollywood_identity` base. `propose setup` validates the runner, model and effort rule up front
just like `backlot hollywood setup`. Applied (`by: user`), the stage runs `hollywood setup`'s
config write (preserving `start` and every other key in `[hollywood]`), records `card.done`,
and restarts Hollywood fresh under the new identity with a takeover first prompt naming what changed.

Actionable card failures (`card.failed`, `card.stale`, `card.cancelled`) reach Hollywood's inbox as
notices; `card.applied` emits no wakeup notice, and `card.done` stays passive context that rides
with the next delivery.

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
  `card.applied` emit no wakeups; queued notices coalesce to `≤ 1` per `(studio, role, label)` and
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
`backlot tell <studio> producer`, `backlot hollywood propose`, `backlot hollywood report`,
`backlot studio new --dry-run`, and the read-only `backlot --help` / `backlot help`, so anything
else it types stops at a prompt you see. That prompt is yours: the stage sends a key only for a
decision you recorded — `y`/`d` on the bar's Hollywood prompt row — and only while the prompt
that decision answered still shows.

The pre-allow list is per-runner: `claude`, `cursor` and `cmd` wire it into the launch argv.
`devin`, `codex` and `opencode` take no command list — there the runner's own permission mode
(`smart`, `on-request`, the env defaults) is the gate, and Hollywood's role rules still refuse at
the ledger.

## What is not here yet

- **H5**: a real agent under an approved run — everything above is runner-agnostic and fake-tested.

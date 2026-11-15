# Hollywood — the overseer

Hollywood is one agent above all studios on a core: the user's assistant for the whole lot. It
reads every studio's ledger, tells you what needs you, reports on any or all studios on request,
and manages studios through **cards** — proposals you confirm. It is not a studio and it is not a
role inside one: it has its own ledger beside `studios/`, in the same format, and studio lists,
limits and studio rules never see it.

This document covers what exists today (H1–H3: the model, the command line, and the stage's live
layer) and marks what lands with the floating layer (H4) and real agents (H5).

## The layer and its key

When the layer is built (H4) it has three states, and it is always a float: it is drawn *over* the
screen and never takes a row or a column from the panes, so toggling it resizes nothing.

- **Hidden** draws nothing. Without a `[hollywood]` section in your config this is the default,
  and the screen is exactly the herdr screen.
- **Collapsed** is a one-row strip floating at the right end of the top row: Hollywood's state,
  studio counts, the number of open cards, and its newest headline.
- **Expanded** is a panel dropping down from the top: one row per studio, Hollywood's last report
  lines, the open cards with Apply / Cancel / Show me, and an input line. At narrow widths it is a
  full-screen view, like the studio panel.

The key is **`alt+y`**, a direct key with no prefix. It switches collapsed and expanded and shows
the layer when hidden; `x` in the expanded panel hides it; `Esc` collapses a focused panel.
`[hollywood] key` changes it (a direct binding like `alt+y`, or `prefix+<key>` for a chord). It is
bound only while `[hollywood]` exists — with no section, Alt+Y reaches the focused program as
always. Known costs, said plainly:

1. While bound, **Alt+Y no longer reaches the program in the pane**. Readline and Emacs use `M-y`
   for yank-pop; if you use it, rebind (`key = "prefix+h"`) or unset the layer.
2. On a terminal **without the kitty keyboard protocol**, Esc followed by `y` inside the escape
   window (150 ms while the mouse is captured) reads as Alt+Y. Rare in practice; the same rebind
   fixes it.
3. On **macOS**, the terminal must send Option as Meta.

The studio panel keeps `prefix+y`.

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

With a valid section the stage runs **one terminal named `bl-hollywood`** — labelled
`backlot.role=hollywood` and never `backlot.studio` — in `<state dir>/hollywood/` with
`BACKLOT_ROLE=hollywood`, the state dir, and no `BACKLOT_STUDIO`. It is started, adopted after a
stage restart, and resumed after a loss the way a producer is (three losses in ten minutes is its
`cannot resume`). Changing the runner starts a new conversation; removing the section closes it.
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

Hollywood's own reports come back through `backlot hollywood report` — a headline the strip and
panel will read from the ledger (D21); today it lands in the summary document's `hollywood`
object. It may also talk *to* a studio's producer on your behalf — its one write into a studio:
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

## What is not here yet

- **H4**: the floating layer, its keys, the input line, Apply / Cancel / Show me in the panel.
- **H5**: a real agent under an approved run — everything above is runner-agnostic and fake-tested.

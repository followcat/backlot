# Hollywood — the overseer

Hollywood is one agent above all studios on a core: the user's assistant for the whole lot. It
reads every studio's ledger, tells you what needs you, reports on any or all studios on request,
and manages studios through **cards** — proposals you confirm. It is not a studio and it is not a
role inside one: it has its own ledger beside `studios/`, in the same format, and studio lists,
limits and studio rules never see it.

This document covers what exists today (H1: the model and the command line) and marks what lands
with the stage loop (H3) and the floating layer (H4).

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
the cast file's rule: `cursor`, `devin` and `cmd` take none; every other runner needs one. Nothing
launches from this yet (H3) — `backlot status --all` and `backlot card list` show the configured
identity meanwhile.

## Cards

A card is Hollywood's only way to change anything. `backlot hollywood propose` records it in
Hollywood's own ledger; you apply or cancel it; only an applied card changes anything — and in H1,
"applying" records the event only. Carrying the action out is the stage's job (H3), and the
command's output says so.

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

Card actions (D18): `studio-new`, `close`, `reopen`, `priority`, `pause`, `resume`, `abandon`.
Nothing else — in particular, no card answers a permission prompt; those stay yours, answered in
the studio.

## Creating a studio in plain words

Tell Hollywood what you want — the goal, the repository, who should work on it — and it writes a
draft in its own folder, checks it with `studio new --dry-run`, and proposes a `studio-new` card
only when the draft passes every check `studio new` runs. Applying the card (H3) creates the
studio; the brief becomes the producer's first message. What you did not say is not guessed: it
comes from `[hollywood] default_cast`, else the last studio's cast, else Hollywood asks.

```sh
backlot studio new --title Shop --repo /home/user/acme-shop --cast /home/user/cast.toml --dry-run --studio acme-shop
backlot hollywood propose studio-new --studio new-shop --title "New shop" --repo /home/user/shop --cast /home/user/hollywood/drafts/new-shop/cast.toml --brief /home/user/hollywood/drafts/new-shop/brief.md --why "user asked" --as hollywood
```

`--dry-run` runs every validation `studio new` runs — the name, the reserved name `hollywood`,
whether the name is taken, whether `--repo` is a git repository, the cast file, the declared
checks, every runner supported *and installed*, and the caps seating the fixed roles — and writes
nothing: no ledger event, no directory. One refusal per kind, each saying what to fix.

## Talking to Hollywood, and what it may say

`backlot tell hollywood` records your line in Hollywood's ledger. In H1 that is the whole story —
delivery to its terminal is the stage's guarded send (H3), never a raw write:

```sh
backlot tell hollywood "watch acme-shop for stalls"
```

Hollywood's own words come back through `backlot hollywood report` — a headline the strip and
panel will read from the ledger (D21). It may also talk *to* a studio's producer on your behalf:
`tell producer` or `ask` sent `--as hollywood` records the studio's `message.queued` as the
delivery record, and mirrors `message.sent` into Hollywood's own ledger.

```sh
backlot hollywood report "two studios idle, one card open" --as hollywood
backlot tell producer --file /home/user/note.md --studio acme-shop --as hollywood
backlot report --node weekly --file /home/user/weekly.md --all --as hollywood
```

## What Hollywood may and may not do

Its role rules, enforced at append:

- **Reads everything**: `status`, `records`, `report --all` and `studio new --dry-run` — with
  `--studio <name>` to pick one studio (`status --all` shows every studio then Hollywood's cards
  and headline).
- **Writes only its own ledger**: card proposals, reports, and the message mirror. Inside a
  studio's ledger the only events it may record are a plain message or a report *to the
  producer* — never to directors, writers or actors, and never any other studio event (R26).
- **Cannot act for you**: `card apply` and `card cancel` refuse whenever `BACKLOT_ROLE` is set —
  a role terminal is never the user. And the stage is never Hollywood's either.
- **`hollywood` is a reserved name**: no studio can take it; its ledger is not a studio.

And the honesty note (D17a): the card gate is a **working rule, not a security boundary**. Identity
in backlot comes from the environment (`BACKLOT_ROLE`, `--as`), so the CLI cannot *prove* an apply
was yours — a role terminal could forge `--as user`. What holds in practice is the pair of rules
above plus the runner's own permission prompt: Hollywood's launch pre-allows only `status`,
`records`, `report`, `tell`, `hollywood propose`, `hollywood report` and `studio new --dry-run`, so
anything else it types stops at a prompt you see.

## What is not here yet

- **H2**: two live studios on one core.
- **H3**: the stage starts `bl-hollywood` from `[hollywood]`, writes `AGENTS.md`/`memory.md`,
  forwards notices, delivers `tell hollywood`, carries applied cards out (`card.done`/`card.failed`),
  and writes the summary document's `hollywood` object.
- **H4**: the floating layer, its keys, the input line, Apply / Cancel / Show me in the panel.

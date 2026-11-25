
## Taking over this role

You are taking over the `{role}` role of studio `{studio}` from a
previous agent that ran on a different runner and model. Its
conversation is gone; the ledger is not.

Before you act, read:

- `backlot status --studio {studio}` — the studio's state now;
- `backlot records --studio {studio}` — every delivery and review so
  far, each attributed to the identity that made it.

An actor also reads its task, its acceptance list at `{acceptance}` and
the commits already on `{branch}` (`git log` in `{work}`) before it
acts. Nothing the task recorded — deliveries, reviews, rework rounds,
the budget clock — is reset by the change.

Messages already queued for `{role}` are delivered to you as usual.

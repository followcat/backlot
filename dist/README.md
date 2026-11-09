# backlot

A terminal UI client for [herdr-core](https://github.com/herdrdev/herdr-core)
(`hc`) — sidebar, tabs and split panes for your hc terminals and agents, on
the local machine or over SSH. Unofficial; not affiliated with the Herdr
project.

The `hc` core must be installed and running separately. Put `backlot` on your
PATH and run it:

```sh
backlot                    # local session $HC_SESSION or "default"
backlot --session work     # named session
backlot --remote host-a    # over SSH
```

Verify this archive against `sha256sums.txt` from the same release before
installing. Project, docs and issue tracker:
<https://github.com/followcat/backlot>. Licensed under the Apache License 2.0
(see `LICENSE` and `NOTICE`).

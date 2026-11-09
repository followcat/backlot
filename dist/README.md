# backlot

A terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`), with a sidebar, tabs and split panes for
your terminals and agents, locally or over SSH. Unofficial, and not affiliated with the Herdr project.

You need `hc` installed — backlot starts the core itself when nothing answers. Put `backlot` on your
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

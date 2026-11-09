# hc-tui

A terminal UI for [herdr-core](https://github.com/herdrdev/herdr-core) (`hc`), with a sidebar, tabs and split panes for
your terminals and agents, locally or over SSH. Unofficial, and not affiliated with the Herdr project.

You need `hc` installed and running. Put `hc-tui` on your PATH and run it:

```sh
hc-tui                    # the session in $HC_SESSION, or "default"
hc-tui --session work
hc-tui --remote host-a    # over SSH
```

Check this archive against `sha256sums.txt` from the same release before installing it.

Docs and issues: <https://github.com/followcat/hc-tui>. Licensed under the Apache License 2.0 (see `LICENSE` and
`NOTICE`).

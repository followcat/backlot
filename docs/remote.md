# Remote use

hc-tui is built for the remote-first setup: an always-on box runs `hc` and every agent; you reach
it over SSH from anywhere, including a phone.

## `hc-tui --remote`

```sh
hc-tui --remote host-a              # hc's default session on host-a
hc-tui --remote host-a --session work
```

`--remote` takes anything your `ssh` accepts: a `~/.ssh/config` host alias, `user@host`,
`host:port` through your ssh config. hc-tui runs `ssh <target> hc connect --session <s>` — the
remote machine needs `hc` on its non-interactive command path (check with `ssh host-a hc -V`).

Authentication is your SSH's: keys, agent, `ProxyJump`, MFA — hc-tui never asks for or stores a
password, and it runs ssh with prompting disabled, so a broken login fails instead of hanging.

Connection costs stay low:

- on Unix, all streams to one target share a single SSH login through hc-tui's control socket
  (`$XDG_RUNTIME_DIR/hc-tui-ssh`, or `/tmp/hc-tui-ssh-<uid>` without it, kept 10 minutes past the
  last stream);
- every attach on a remote endpoint multiplexes onto one framed `hc connect` channel each —
  one transport, one handshake.

The session and the terminals stay on the server. Detach (`Ctrl+B q`), lose your connection, or
close your laptop — the core and agents keep running; attach again later from the same or another
client. Two hc-tui clients on the same core even share splits and tabs: layout state lives in the
core's `hc-tui` document. See [runtime.md](runtime.md) for the full behaviour.

## `hc machine add` — more machines on one core

`hc machine add` teaches an existing core about *other* machines over SSH. Those machines' terminals
appear in the same hc-tui sidebar, grouped under their machine — you drive one core and see all of
them. Manage machines on the core itself (`hc machine add|ls|login|rename|rm`), not from hc-tui.

So the two axes are:

- `--remote` picks **which core** your client talks to;
- `hc machine add` decides **which machines that core knows about**.

The [compatibility table](compatibility.md) applies per machine: a sidebar can mix fork and official
cores, each with its own capabilities.

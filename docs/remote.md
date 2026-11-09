# Remote use

You can run `hc` and your agents on a server and connect to it over SSH.

## `hc-tui --remote`

```sh
hc-tui --remote host-a              # the default session on host-a
hc-tui --remote host-a --session work
```

`--remote` takes anything your `ssh` accepts, such as a host alias from `~/.ssh/config` or `user@host`. hc-tui runs
`ssh <target> hc connect --session <s>`, so `hc` has to be on the remote machine's non-interactive `PATH`. Check with
`ssh host-a hc -V`.

Logins use your normal SSH setup, including keys, your agent and `ProxyJump`. hc-tui never asks for or stores a
password. It runs ssh with prompts turned off, so a login that needs one fails right away.

All connections to a target share one SSH login, through a control socket in `$XDG_RUNTIME_DIR/hc-tui-ssh`, or
`/tmp/hc-tui-ssh-<uid>` if that variable isn't set. The login stays open for 10 minutes after the last connection
closes.

Terminals and agents stay on the server. If you detach (`Ctrl+B q`) or lose the connection, they keep running and you
can attach again later, from any client. Clients on the same core share tabs and splits, because the layout is stored
on the core. [Runtime](runtime.md) has the details.

## Pasting text and images

`Ctrl+Shift+V` pastes text. The program receives it as a bracketed paste, unchanged.

`Ctrl+V` pastes a clipboard image. hc-tui reads your local clipboard, uploads the image to the remote core with
`terminal.put`, and the program receives the path of the uploaded file. The same applies to a focused terminal on
another machine (`machine/tN`) and to hc-tui itself running inside an SSH session. On a plain local endpoint `Ctrl+V`
is left alone. Local programs read the clipboard themselves.

On Linux the clipboard read needs `wl-paste` under Wayland or `xclip` under X11. A missing tool counts as no image.
PNG, JPEG, GIF, WebP and BMP are recognized, up to the 8 MB `terminal.put` limit. The key is
`keys.remote_image_paste` (default `"ctrl+v"`). Set it to `""` to disable the trigger.
[Keys](keys.md#pasting-text-and-images) covers the trigger config and the other gestures.

## More machines with `hc machine add`

`hc machine add` connects a core to other machines over SSH. Their terminals appear in the same sidebar, grouped by
machine. Manage them on the core with `hc machine add|ls|login|rename|rm`. hc-tui doesn't do this for you.

`--remote` chooses which core your client connects to. `hc machine add` chooses which machines that core can reach.

A sidebar can mix fork and official cores. The [compatibility table](compatibility.md) applies to each machine
separately.

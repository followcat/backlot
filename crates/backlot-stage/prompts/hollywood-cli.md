### Command reference

Generated from the CLI's own usage strings. Use it; run `backlot --help`
only for a command it does not list. Read-only anywhere: `backlot help`,
`backlot <command> --help`.

- `backlot status [--studio <name>] [--all] [--json]`
  flags: --studio --all --json

- `backlot records [--studio <name>] [--all] [--kind <task_kind>] [--json]`
  flags: --studio --all --kind --json

- `backlot tell producer (--file <f> | --text <msg> | --stdin) [--ask]`
  flags: --file --text --stdin --ask

- `backlot progress (<line> | --file <f> | --text <msg> | --stdin)`
  flags: --file --text --stdin

- `backlot studio new --title <t> --repo <path> --cast <file.toml> [--ask <text> | --ask-file <f>] [--mode standard|experimental] [--dry-run]`
  flags: --title --repo --cast --ask --ask-file --mode --dry-run

- `backlot hollywood propose <action> [--studio <s>] [--task <t>] [--person <role>] [--level low|normal|high] [--runner <r>] [--model <m>] [--effort <e>] [--title <t>] [--repo <path>] [--cast <f>] [--brief <f>] [--fresh] [--file <f>] [--answers <seqs>] --why <line>`
  flags: --studio --task --person --level --runner --model --effort --title --repo --cast --brief --fresh --file --answers --why
  actions: studio-new | close | reopen | reopen-studio | priority | pause | resume | abandon | recast | restart | cast | setup
  example (`propose setup`): `backlot hollywood propose setup --runner claude --model claude-opus-5-5 --effort xhigh --why "switch agent"`
  examples: `propose restart --studio <s> --person <key> [--fresh] --why "stuck"`,
  `propose cast --studio <s> --file drafts/<studio>/cast.toml --why "cheaper actors"`

- `backlot hollywood report "<headline>" [--body <f>] [--answers <seqs>]`
  flags: --body --answers

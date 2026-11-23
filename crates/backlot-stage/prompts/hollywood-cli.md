### Command reference

Generated from the CLI's own usage strings. Use it; run `backlot --help`
only for a command it does not list. Read-only anywhere: `backlot help`,
`backlot <command> --help`.

- `backlot status [--studio <name>] [--all] [--json]`
  flags: --studio --all --json

- `backlot records [--studio <name>] [--all] [--kind <task_kind>] [--json]`
  flags: --studio --all --kind --json

- `backlot tell producer (--file <f> | --text <msg> | --stdin)`
  flags: --file --text --stdin

- `backlot studio new --title <t> --repo <path> --cast <file.toml> [--mode standard|experimental] [--dry-run]`
  flags: --title --repo --cast --mode --dry-run

- `backlot hollywood propose <action> [--studio <s>] [--task <t>] [--person <role>] [--level low|normal|high] [--runner <r>] [--model <m>] [--effort <e>] [--title <t>] [--repo <path>] [--cast <f>] [--brief <f>] --why <line>`
  flags: --studio --task --person --level --runner --model --effort --title --repo --cast --brief --why

- `backlot hollywood report "<headline>" [--body <f>]`
  flags: --body

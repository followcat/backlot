//! Every command's `usage:` line — the parsers pass theirs to
//! `args::usage`, which appends `args::COMMON`.

pub(crate) const U_STUDIO: &str = "usage: backlot studio new --title <t> --repo <path> --cast <file.toml> [--ask <text> | --ask-file <f>] [--mode standard|experimental] [--dry-run]\n       backlot studio new --interactive [--title <t>] [--repo <path>] [--cast <file.toml>] [--ask <text> | --ask-file <f>] [--mode standard|experimental]\n       backlot studio finish [--reason <r>] [--force]\n       backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]\n       backlot studio delete [<studio>] [--dry-run] [--force] [--json]\n       backlot studio migrate [<studio> | --all] [--dry-run]\n       backlot studio list [--json] [--archived]\n       backlot studio restore <archived-name> [--name <new-name>] [--dry-run]";
pub(crate) const U_STUDIO_LIST: &str = "usage: backlot studio list [--json] [--archived]\n       backlot --remote <host> studio list [--json]\n  studio list reads the local state directory (every local core) by default.\n  --archived lists the ledgers under archive/ instead — name, stamp, repo and\n  last requirement title — and `studio restore <archived-name>` brings one back.\n  --remote <host> lists the studios on <host>, for example\n  `backlot --remote host-b studio list`.";
pub(crate) const U_REQ: &str = "usage: backlot req revise (--file <f> | --stdin) --summary <s>";
pub(crate) const U_PLAN: &str =
    "usage: backlot plan request | publish --kind plan|acceptance --file <f>";
pub(crate) const U_DISPATCH: &str = "usage: backlot dispatch --file <f>";
pub(crate) const U_REPORT: &str = "usage: backlot report --node <n> --file <f>\n       backlot report show --studio <s> --file <relpath>";
pub(crate) const U_ASK: &str = "usage: backlot ask --file <f>\n       backlot ask --run [--why <text>] (-- <prog> <args...> | \"<cmd>\")";
pub(crate) const U_TELL: &str = "usage: backlot tell <role> (--file <f> | --text <msg> | --stdin)\n       backlot tell hollywood \"<text>\" [--text <msg>]";
pub(crate) const U_PROGRESS: &str =
    "usage: backlot progress (<line> | --file <f> | --text <msg> | --stdin)";
pub(crate) const U_TASK: &str = "usage: backlot task new --title <t> --kind <k> --acceptance-file <f> [--depends-on <id>...]\n       backlot task pause|resume|abandon|takeover --task <id> --reason <r>";
pub(crate) const U_CAST: &str = "usage: backlot cast --task <id> --actor <name> [--reason <r>] [--priority low|normal|high]\n       backlot cast --task <id> --suggest [--json]";
pub(crate) const U_PRIORITY: &str = "usage: backlot priority --task <id> --level low|normal|high";
pub(crate) const U_REOPEN: &str = "usage: backlot reopen --person <role key>";
pub(crate) const U_CLOSE: &str = "usage: backlot close --person <role key>\n  close releases a held person (reopen's undo). The producer, director and writer\n  are fixed seats: restart one, or finish the studio.";
pub(crate) const U_RESTART: &str = "usage: backlot restart --person <role key> [--fresh] [--reason <r>]\n  restart stops the person's terminal and launches it again under the same\n  identity. It resumes the recorded session when the runner supports resume by\n  id; --fresh starts a new session with the role's prompt.";
pub(crate) const U_RECAST: &str = "usage: backlot recast --person producer|director|writer --runner <r> --model <m> [--effort <e>]\n       [--permission-profile standard|worktree] [--reason <r>] [--studio <s>]";
pub(crate) const U_APPROVE: &str =
    "usage: backlot approve --person <role key> --decision allow|deny";
pub(crate) const U_ATTENTION: &str = "usage: backlot attention clear <seq>";
pub(crate) const U_DELIVER: &str = "usage: backlot deliver --commits A..B --report <f> [--check name=pass|fail...]\n       [--tokens-in <n>] [--tokens-out <n>] [--cache-read-tokens <n>] [--cost-usd <d>]";
pub(crate) const U_BLOCK: &str = "usage: backlot block --reason <r>";
pub(crate) const U_REVIEW: &str = "usage: backlot review --task <id> --delivery <n> --verdict \
accepted|partially-accepted|changes-requested|blocked --conclusion <text> \
[--findings <f.json>] [--upstream-changes <f.json>]";
pub(crate) const U_FINDING: &str =
    "usage: backlot finding --task <id> --delivery <n> --findings <file.json>";
pub(crate) const U_CORRECT: &str =
    "usage: backlot correct --event <seq> --reason <r> --set <key=value> [--set ...]";
pub(crate) const U_STATUS: &str = "usage: backlot status [--studio <name>] [--all] [--json]";
pub(crate) const U_RECORDS: &str =
    "usage: backlot records [--studio <name>] [--all] [--kind <task_kind>] [--json]";
pub(crate) const U_STAGE: &str = "usage: backlot stage start|stop [--session <s>] [--remote <t>]\n       backlot stage run\n       backlot stage watch [--target <hc-terminal>] [--restart]\n       backlot stage service [--print | --write] [--session <s>]";
pub(crate) const U_SERVER: &str = "usage: backlot server run [--session <s>]\n       backlot server start|stop [--session <s>] [--remote <t>]\n       backlot server adopt [--dry-run] [--stop-stages] [--core <id>]\n       backlot server service [--print | --write] [--session <s>]";
pub(crate) const U_RUNNERS: &str = "usage: backlot runners [--json]";
pub(crate) const U_REPOS: &str = "usage: backlot repos [--json] [--from <dir>] [--all]\n       dot-directories stay hidden unless --all is given";

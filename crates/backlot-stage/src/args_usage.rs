//! Every command's `usage:` line — the parsers pass theirs to
//! `args::usage`, which appends `args::COMMON`.

pub(crate) const U_STUDIO: &str = "usage: backlot studio new --title <t> --repo <path> --cast <file.toml> [--dry-run]\n       backlot studio new --interactive [--title <t>] [--repo <path>] [--cast <file.toml>]\n       backlot studio finish [--reason <r>]";
pub(crate) const U_REQ: &str = "usage: backlot req revise (--file <f> | --stdin) --summary <s>";
pub(crate) const U_PLAN: &str =
    "usage: backlot plan request | publish --kind plan|acceptance --file <f>";
pub(crate) const U_DISPATCH: &str = "usage: backlot dispatch --file <f>";
pub(crate) const U_REPORT: &str = "usage: backlot report --node <n> --file <f>\n       backlot report show --studio <s> --file <relpath>";
pub(crate) const U_ASK: &str = "usage: backlot ask --file <f>";
pub(crate) const U_TELL: &str = "usage: backlot tell <role> (--file <f> | --text <msg> | --stdin)\n       backlot tell hollywood \"<text>\" [--text <msg>]";
pub(crate) const U_TASK: &str = "usage: backlot task new --title <t> --kind <k> --acceptance-file <f> [--depends-on <id>...]\n       backlot task pause|resume|abandon|takeover --task <id> --reason <r>";
pub(crate) const U_CAST: &str =
    "usage: backlot cast --task <id> --actor <name> [--reason <r>] [--priority low|normal|high]";
pub(crate) const U_PRIORITY: &str = "usage: backlot priority --task <id> --level low|normal|high";
pub(crate) const U_REOPEN: &str = "usage: backlot reopen --person <role key>";
pub(crate) const U_CLOSE: &str = "usage: backlot close --person <role key>";
pub(crate) const U_APPROVE: &str =
    "usage: backlot approve --person <role key> --decision allow|deny";
pub(crate) const U_ATTENTION: &str = "usage: backlot attention clear <seq>";
pub(crate) const U_DELIVER: &str =
    "usage: backlot deliver --commits A..B --report <f> [--check name=pass|fail...]";
pub(crate) const U_BLOCK: &str = "usage: backlot block --reason <r>";
pub(crate) const U_REVIEW: &str = "usage: backlot review --task <id> --delivery <n> --verdict \
accepted|partially-accepted|changes-requested|blocked --conclusion <text> \
[--findings <f.json>] [--upstream-changes <f.json>]";
pub(crate) const U_FINDING: &str =
    "usage: backlot finding --task <id> --delivery <n> --findings <file.json>";
pub(crate) const U_CORRECT: &str =
    "usage: backlot correct --event <seq> --reason <r> --set <key=value> [--set ...]";
pub(crate) const U_STATUS: &str = "usage: backlot status [--studio <name>] [--all] [--json]";
pub(crate) const U_RECORDS: &str = "usage: backlot records [--studio <name>] [--all] [--json]";
pub(crate) const U_STAGE: &str = "usage: backlot stage start|stop [--session <s>] [--remote <t>]\n       backlot stage run\n       backlot stage watch [--target <hc-terminal>]";
pub(crate) const U_RUNNERS: &str = "usage: backlot runners [--json]";
pub(crate) const U_REPOS: &str = "usage: backlot repos [--json] [--from <dir>]";

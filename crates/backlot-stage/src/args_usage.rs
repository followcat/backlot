//! Every command's `usage:` line — the parsers pass theirs to
//! `args::usage`, which appends `args::COMMON`.

pub(crate) const U_STUDIO: &str = "usage: backlot studio new --title <t> --repo <path> --cast <file.toml> [--ask <text> | --ask-file <f>] [--mode standard|experimental] [--dry-run]\n       backlot studio new --interactive [--title <t>] [--repo <path>] [--cast <file.toml>] [--ask <text> | --ask-file <f>] [--mode standard|experimental]\n       backlot studio cast show [--json]\n       backlot studio cast set --file <file.toml> [--dry-run] [--json]\n       backlot studio check add --name <n> --run <cmd> [--timeout <s>] [--expect <re>] [--min-matches <k>] [--preflight]\n       backlot studio check remove --name <n>\n       backlot studio finish [--reason <r>] [--force]\n       backlot studio reopen [--reason <r>]\n       backlot studio prune [<studio> | --all-finished] [--dry-run] [--force]\n       backlot studio delete [<studio>] [--dry-run] [--force] [--json]\n       backlot studio migrate [<studio> | --all] [--dry-run]\n       backlot studio list [--json] [--archived]\n       backlot studio restore <archived-name> [--name <new-name>] [--dry-run]\n  studio new needs a name matching [a-z0-9_-]{1,16}. reopen brings a finished studio\n  back for recasts, cast changes and new tasks; it is the person's command.";
pub(crate) const U_STUDIO_CAST: &str = "usage: backlot studio cast show [--json]\n       backlot studio cast set --file <file.toml> [--dry-run] [--json]\n  studio cast changes a studio's cast after it started: the cast file given to\n  `studio new` only initializes it. `show` prints the effective cast as a cast\n  file; `set` validates a cast file with `studio new --dry-run`'s runnable rules\n  (the roots, the effort and cline-model rules, positive limits, and caps that\n  still seat the fixed roles) and, unless --dry-run, records it. A fixed role\n  whose identity changes is recast the same way `backlot recast` does. Live\n  actors keep running; new limits apply to the next start; new checks run on the\n  next delivery. A role terminal is refused; a finished studio needs reopen or\n  restore first.";
pub(crate) const U_STUDIO_CHECK: &str = "usage: backlot studio check add --name <n> --run <cmd> [--timeout <s>] [--expect <re>] [--min-matches <k>] [--preflight]\n       backlot studio check remove --name <n>\n  User-only: add or remove one stage check in `studio.cast`. Checks added\n  here survive later opening-design re-signings.";
pub(crate) const U_STUDIO_LIST: &str = "usage: backlot studio list [--json] [--archived]\n       backlot --remote <host> studio list [--json]\n  studio list reads the local state directory (every local core) by default.\n  --archived lists the ledgers under archive/ instead — name, stamp, repo and\n  last requirement title — and `studio restore <archived-name>` brings one back.\n  --remote <host> lists the studios on <host>, for example\n  `backlot --remote host-b studio list`.";
pub(crate) const U_REQ: &str = "usage: backlot req revise (--file <f> | --stdin) --summary <s>";
pub(crate) const U_PLAN: &str =
    "usage: backlot plan request | publish --kind plan|acceptance --file <f>";
pub(crate) const U_DISPATCH: &str = "usage: backlot dispatch --file <f>";
pub(crate) const U_REPORT: &str = "usage: backlot report --node <n> --file <f>\n       backlot report show --studio <s> --file <relpath>";
pub(crate) const U_ASK: &str = "usage: backlot ask --file <f>
       backlot ask acknowledged --person <role> --target <seq>\n       backlot ask --run [--why <text>] (-- <prog> <args...> | \"<cmd>\")";
pub(crate) const U_TELL: &str = "usage: backlot tell <role> (--file <f> | --text <msg> | --stdin) [--ask | --note]\n       backlot tell hollywood \"<text>\" [--text <msg>]";
pub(crate) const U_DECIDE: &str = "usage: backlot decide --id AD-<n> --file <path> [--supersedes AD-<m>] [--notify <roles>|active-actors]";
pub(crate) const U_REPO_NOTE: &str = "usage: backlot repo-note propose --id <slug> --text <text> --why <reason>\n       backlot repo-note show [--repo <path>]\n       backlot repo-note remove --id <slug> [--repo <path>]";
pub(crate) const U_MSG: &str = "usage: backlot msg show <seq> [--json]";
pub(crate) const U_PROGRESS: &str =
    "usage: backlot progress (<line> | --file <f> | --text <msg> | --stdin)";
pub(crate) const U_TASK: &str = "usage: backlot task new --title <t> --kind <k> --acceptance-file <f> [--depends-on <id>...]\n       backlot task pause|resume|abandon|takeover --task <id> --reason <r>";
pub(crate) const U_CAST: &str = "usage: backlot cast --task <id> --actor <name> [--reason <r>] [--priority low|normal|high]\n       backlot cast --task <id> --suggest [--json]";
pub(crate) const U_PRIORITY: &str = "usage: backlot priority --task <id> --level low|normal|high";
pub(crate) const U_REOPEN: &str = "usage: backlot reopen --person <role key>";
pub(crate) const U_CLOSE: &str = "usage: backlot close --person <role key>\n  close releases a held person (reopen's undo). The producer, director and writer\n  are fixed seats: restart one, or finish the studio.";
pub(crate) const U_RESTART: &str = "usage: backlot restart --person <role key> [--fresh] [--reason <r>]\n  restart stops the person's terminal and launches it again under the same\n  identity. It resumes the recorded session when the runner supports resume by\n  id; --fresh starts a new session with the role's prompt.";
pub(crate) const U_RECAST: &str = "usage: backlot recast --person producer|director|writer | actor-<task>-c<n>\n       --runner <r> --model <m> [--effort <e>]\n       [--permission-profile standard|worktree] [--reason <r>] [--studio <s>]\n  A fixed role swaps the agent that plays it; an actor key swaps the actor on its\n  task (its newest cast's key). Either way the terminal name and the work area\n  stay, the conversation is new, and the task's deliveries and budget stay.";
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
pub(crate) const U_METRICS: &str =
    "usage: backlot metrics [--studio <id> | --all] [--since <dur>] [--json]";
pub(crate) const U_DIGEST: &str =
    "usage: backlot digest [--studio <id> | --all] [--since <dur>] [--format md|json]";
pub(crate) const U_STAGE: &str = "usage: backlot stage start|stop [--session <s>] [--remote <t>]\n       backlot stage run\n       backlot stage watch [--target <hc-terminal>] [--restart]\n       backlot stage service [--print | --write] [--session <s>]";
pub(crate) const U_SERVER: &str = "usage: backlot server run [--session <s>]\n       backlot server start|stop [--session <s>] [--remote <t>]\n       backlot server adopt [--dry-run] [--stop-stages] [--core <id>]\n       backlot server service [--print | --write] [--session <s>]";
pub(crate) const U_GUIDE: &str = "usage: backlot guide [<topic>] [--grep <word>] — backlot's own docs, compiled into the binary (no studio, no disk search)";
pub(crate) const U_RUNNERS: &str = "usage: backlot runners [--json]";
pub(crate) const U_REPOS: &str = "usage: backlot repos [--json] [--from <dir>] [--all]\n       dot-directories stay hidden unless --all is given";
pub(crate) const U_DESIGN: &str = "usage: backlot design open [--req <v>] [--suggest-skip [--reason <r>]]\n       backlot design write --slot <proposal|buildability|summary> --file <p> [--refs <seq>[,<seq>...]] [--question <json|@file>...] [--questions-file <p>] [--checks-file <toml>]\n       backlot design supplement --file <p> --refs <seq>[,<seq>...]\n       backlot design note --from user (--text <t> | --file <p> | --stdin)\n       backlot design next\n       backlot design sign [--skip [--reason <r>]] [--note <t>] [--no-checks] [--as user]\n       backlot design show [--version <n>] [--round <r>] [--json]";

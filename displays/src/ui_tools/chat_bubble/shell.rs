//! Classified runs of a shell command line, for colouring it.

use eframe::egui::{Color32, FontId, TextFormat, text::LayoutJob};

/// What a run of a command line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// First word of each simple command.
    Command,
    /// `-x`, `--long`, `--key=`.
    Flag,
    /// A quoted string, quotes included.
    Str,
    Arg,
    /// `|`, `&&`, `;`, redirections.
    Op,
    Num,
    /// `NAME=value` ahead of the command.
    Assign,
    Space,
}

/// Colour per run class.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShellColors {
    pub command: Color32,
    pub flag: Color32,
    pub string: Color32,
    pub number: Color32,
    pub operator: Color32,
    pub base: Color32,
}

/// Splits a command line into classified runs that concatenate back to the input.
pub(crate) fn spans(cmd: &str) -> Vec<(Class, String)> {
    let b: Vec<char> = cmd.chars().collect();
    let mut out: Vec<(Class, String)> = Vec::new();
    let mut i = 0;
    let mut expect_command = true;
    let mut continued = false;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            let start = i;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            let s: String = b[start..i].iter().collect();
            // A newline starts a new command unless the previous line ended in a backslash.
            if s.contains('\n') && !continued {
                expect_command = true;
            }
            continued = false;
            out.push((Class::Space, s));
            continue;
        }
        if c == '"' || c == '\'' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != c {
                if c == '"' && b[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            out.push((Class::Str, b[start..i].iter().collect()));
            expect_command = false;
            continue;
        }
        if c == '\\' && b.get(i + 1).is_none_or(|n| *n == '\n') {
            out.push((Class::Op, "\\".into()));
            continued = true;
            i += 1;
            continue;
        }
        if let Some((n, chains)) = operator(&b[i..]) {
            out.push((Class::Op, b[i..i + n].iter().collect()));
            i += n;
            if chains {
                expect_command = true;
            }
            continue;
        }
        let start = i;
        while i < b.len() && !b[i].is_whitespace() && !is_break(b[i]) {
            i += 1;
        }
        if i == start {
            out.push((Class::Arg, b[i].to_string()));
            i += 1;
            continue;
        }
        let w: String = b[start..i].iter().collect();
        let class = if expect_command && is_assignment(&w) {
            Class::Assign
        } else if expect_command {
            expect_command = false;
            Class::Command
        } else if w.starts_with('-') && w.len() > 1 {
            Class::Flag
        } else if w.parse::<f64>().is_ok() {
            Class::Num
        } else {
            Class::Arg
        };
        out.push((class, w));
    }
    out
}

/// Characters a bare word stops at.
fn is_break(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '|' | '&' | ';' | '>' | '<' | '(' | ')' | '`'
    )
}

/// The operator at the head of `s` as (length, whether a new command follows it).
fn operator(s: &[char]) -> Option<(usize, bool)> {
    const OPERATORS: [(&str, bool); 12] = [
        ("2>&1", false),
        ("||", true),
        ("&&", true),
        (">>", false),
        ("|", true),
        (";", true),
        (">", false),
        ("<", false),
        ("&", true),
        ("(", true),
        (")", false),
        ("`", true),
    ];
    OPERATORS.iter().find_map(|(p, chains)| {
        let len = p.chars().count();
        (s.len() >= len && s.iter().zip(p.chars()).all(|(a, b)| *a == b)).then_some((len, *chains))
    })
}

/// True for `NAME=value` with a plain identifier before the `=`.
fn is_assignment(w: &str) -> bool {
    let Some((name, _)) = w.split_once('=') else {
        return false;
    };
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Appends a command line to a layout job, one colour per run class.
pub(crate) fn append(job: &mut LayoutJob, cmd: &str, font: &FontId, colors: &ShellColors) {
    for (class, text) in spans(cmd) {
        let color = match class {
            Class::Command => colors.command,
            Class::Flag => colors.flag,
            Class::Str => colors.string,
            Class::Num => colors.number,
            Class::Op | Class::Assign => colors.operator,
            Class::Arg | Class::Space => colors.base,
        };
        job.append(
            &text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
}

/// True for a JSON key whose string value is a command line.
pub(crate) fn is_command_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "command"
            | "cmd"
            | "commands"
            | "script"
            | "shell"
            | "bash"
            | "sh"
            | "zsh"
            | "exec"
            | "run"
    )
}

/// True for a code fence language that is a shell.
pub(crate) fn is_shell_lang(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "bash" | "sh" | "zsh" | "shell" | "console" | "fish"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(cmd: &str) -> Vec<(Class, String)> {
        spans(cmd)
            .into_iter()
            .filter(|(c, _)| *c != Class::Space)
            .collect()
    }

    fn pairs(v: &[(Class, &str)]) -> Vec<(Class, String)> {
        v.iter().map(|(c, s)| (*c, s.to_string())).collect()
    }

    #[test]
    fn runs_concatenate_back_to_the_input() {
        for cmd in [
            "ls -la /tmp",
            "a | b && c; d",
            "x \"unterminated",
            "  spaced   out  \n next",
        ] {
            let back: String = spans(cmd).into_iter().map(|(_, s)| s).collect();
            assert_eq!(back, cmd);
        }
    }

    #[test]
    fn the_program_its_flags_and_its_strings_are_told_apart() {
        assert_eq!(
            classes("FOO=1 winget install --id=\"Git.Git\" -e --scope machine 2"),
            pairs(&[
                (Class::Assign, "FOO=1"),
                (Class::Command, "winget"),
                (Class::Arg, "install"),
                (Class::Flag, "--id="),
                (Class::Str, "\"Git.Git\""),
                (Class::Flag, "-e"),
                (Class::Flag, "--scope"),
                (Class::Arg, "machine"),
                (Class::Num, "2"),
            ])
        );
    }

    #[test]
    fn each_command_in_a_pipeline_or_chain_is_a_command() {
        assert_eq!(
            classes("cat x | grep -i 'q' && echo done; ls"),
            pairs(&[
                (Class::Command, "cat"),
                (Class::Arg, "x"),
                (Class::Op, "|"),
                (Class::Command, "grep"),
                (Class::Flag, "-i"),
                (Class::Str, "'q'"),
                (Class::Op, "&&"),
                (Class::Command, "echo"),
                (Class::Arg, "done"),
                (Class::Op, ";"),
                (Class::Command, "ls"),
            ])
        );
    }

    #[test]
    fn redirection_targets_are_arguments_and_a_backslash_continues_the_command() {
        assert_eq!(
            classes("make > build.log 2>&1"),
            pairs(&[
                (Class::Command, "make"),
                (Class::Op, ">"),
                (Class::Arg, "build.log"),
                (Class::Op, "2>&1")
            ])
        );
        assert_eq!(
            classes("render \\\n  --size 2\nls"),
            pairs(&[
                (Class::Command, "render"),
                (Class::Op, "\\"),
                (Class::Flag, "--size"),
                (Class::Num, "2"),
                (Class::Command, "ls"),
            ])
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string_and_an_open_one_runs_to_the_end() {
        assert_eq!(
            classes("say \"he said \\\"hi\\\"\" now")[1],
            (Class::Str, "\"he said \\\"hi\\\"\"".to_string())
        );
        assert_eq!(classes("x 'open")[1], (Class::Str, "'open".to_string()));
    }

    #[test]
    fn command_keys_and_shell_fences_are_recognised() {
        assert!(is_command_key("command"));
        assert!(is_command_key("Cmd"));
        assert!(!is_command_key("prompt"));
        assert!(is_shell_lang("bash"));
        assert!(is_shell_lang(" zsh "));
        assert!(!is_shell_lang("rust"));
    }
}

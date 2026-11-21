// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

/// Parses a single-line input shell argument string into an argument list,
/// supporting single-quote and double-quote pairing as well as backslash escaping (similar to Shell lexical parsing)
pub fn parse_shell_args(raw: &str) -> Vec<String> {
    #[derive(Clone, Copy)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut args = Vec::new();
    let mut current = String::new();

    let mut in_token = false;
    let mut quote = Quote::None;
    let mut chars = raw.chars();

    while let Some(c) = chars.next() {
        match (quote, c) {
            // Unquoted whitespace: finish the current argument (if any).
            (Quote::None, c) if c.is_whitespace() => {
                if in_token {
                    args.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }

            // Opening quotes.
            (Quote::None, '\'') => {
                quote = Quote::Single;
                in_token = true;
            }
            (Quote::None, '"') => {
                quote = Quote::Double;
                in_token = true;
            }

            // Backslash outside quotes: take the next character literally.
            (Quote::None, '\\') => {
                current.push(chars.next().unwrap_or('\\'));
                in_token = true;
            }

            // Closing quotes.
            (Quote::Single, '\'') | (Quote::Double, '"') => {
                quote = Quote::None;
            }

            // Backslash inside double quotes: only `\"` and `\\` are escapes.
            (Quote::Double, '\\') => match chars.next() {
                Some(next @ ('"' | '\\')) => current.push(next),
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => current.push('\\'),
            },

            // Any other character is part of the current argument.
            (_, c) => {
                current.push(c);
                in_token = true;
            }
        }
    }

    if in_token {
        args.push(current);
    }

    args
}

/// Formats a list of shell arguments into a single-line string suitable for
/// shell-like lexical parsing via [`parse_shell_args`].
pub fn format_shell_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            let needs_quotes = arg.is_empty()
                || arg
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\');

            if needs_quotes {
                let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses a shell-like `KEY=VALUE` string into an ordered map of environment variables.
/// Supports quotes, backslash escaping, spaces, and newlines via [`parse_shell_args`].
/// Invalid tokens (missing `=`, empty keys, or keys containing whitespace/NUL) are skipped.
pub fn parse_env_vars(raw: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    for token in parse_shell_args(raw) {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };

        if key.is_empty()
            || key.chars().any(|c| c.is_whitespace() || c == '\0')
            || value.contains('\0')
        {
            continue;
        }

        vars.insert(key.to_string(), value.to_string());
    }

    vars
}

/// Formats an environment variable map into a single-line `KEY=VALUE` string
/// compatible with [`parse_env_vars`].
pub fn format_env_vars(vars: &BTreeMap<String, String>) -> String {
    vars.iter()
        .map(|(key, value)| {
            let needs_quotes = value
                .chars()
                .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\');

            if needs_quotes {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                format!("{key}=\"{escaped}\"")
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shell_args_empty_and_whitespace() {
        assert_eq!(parse_shell_args(""), Vec::<String>::new());
        assert_eq!(parse_shell_args("   \t\n\r  "), Vec::<String>::new());
    }

    #[test]
    fn test_parse_shell_args_basic() {
        assert_eq!(parse_shell_args("-Xmx2G -Xms1G"), vec!["-Xmx2G", "-Xms1G"]);
        assert_eq!(
            parse_shell_args("  -Xmx2G    -Xms1G  "),
            vec!["-Xmx2G", "-Xms1G"]
        );
    }

    #[test]
    fn test_parse_shell_args_quotes_and_escapes() {
        assert_eq!(
            parse_shell_args(r#"-Dfoo="bar baz" -Dtest='hello world'"#),
            vec!["-Dfoo=bar baz", "-Dtest=hello world"]
        );
        assert_eq!(
            parse_shell_args(r#"-Dpath="C:\\Program Files\\App""#),
            vec![r"-Dpath=C:\Program Files\App"]
        );
        assert_eq!(
            parse_shell_args(r#"-Dquote="\"escaped\"""#),
            vec![r#"-Dquote="escaped""#]
        );
        assert_eq!(parse_shell_args(r"-Dfoo=bar\ baz"), vec!["-Dfoo=bar baz"]);
    }

    #[test]
    fn test_parse_shell_args_unclosed_quotes_and_trailing_backslash() {
        assert_eq!(parse_shell_args(r#"-Dfoo="bar"#), vec!["-Dfoo=bar"]);
        assert_eq!(parse_shell_args(r"-Dfoo=bar\"), vec![r"-Dfoo=bar\"]);
        assert_eq!(parse_shell_args(r#"-Dfoo="bar\"#), vec![r"-Dfoo=bar\"]);
    }

    #[test]
    fn test_format_shell_args_basic() {
        assert_eq!(
            format_shell_args(&["-Xmx2G".to_string(), "-Xms1G".to_string()]),
            "-Xmx2G -Xms1G"
        );
        assert_eq!(
            format_shell_args(&[
                "-Dfoo=bar baz".to_string(),
                r"-Dpath=C:\test\app".to_string(),
                "".to_string(),
            ]),
            r#""-Dfoo=bar baz" "-Dpath=C:\\test\\app" """#
        );
    }

    #[test]
    fn test_shell_args_roundtrip() {
        let test_cases = vec![
            vec!["-Xmx2G".to_string(), "-Xms1G".to_string()],
            vec![r"-Dpath=C:\test\app".to_string(), "".to_string()],
            vec![
                r"-Dmessage=it's ok".to_string(),
                r#"-Dname="hello""#.to_string(),
            ],
            vec!["".to_string(), "foo".to_string(), "".to_string()],
            vec![
                r"a\b\c".to_string(),
                "hello world".to_string(),
                "tab\there".to_string(),
            ],
            vec![r#"foo "bar" baz"#.to_string()],
            vec!["--add-opens=java.base/java.lang=ALL-UNNAMED".to_string()],
        ];

        for original in test_cases {
            let formatted = format_shell_args(&original);
            let parsed = parse_shell_args(&formatted);
            assert_eq!(
                original, parsed,
                "Roundtrip failed for formatted string: {formatted}"
            );
        }
    }

    #[test]
    fn test_parse_env_vars_basic_and_quotes() {
        assert_eq!(parse_env_vars(""), BTreeMap::new());
        assert_eq!(parse_env_vars("   \t\n  "), BTreeMap::new());

        let parsed = parse_env_vars(
            r#"FOO=bar BAZ="hello world" SINGLE='it works' OPT=a=b=c EMPTY= DUP=1 DUP=2
MULTI="line two" ESC="\"q\"\\s""#,
        );
        let expected = BTreeMap::from([
            ("BAZ".to_string(), "hello world".to_string()),
            ("DUP".to_string(), "2".to_string()),
            ("EMPTY".to_string(), "".to_string()),
            ("ESC".to_string(), r#""q"\s"#.to_string()),
            ("FOO".to_string(), "bar".to_string()),
            ("MULTI".to_string(), "line two".to_string()),
            ("OPT".to_string(), "a=b=c".to_string()),
            ("SINGLE".to_string(), "it works".to_string()),
        ]);
        assert_eq!(parsed, expected);
    }

    #[test]
    fn test_parse_env_vars_ignores_invalid_tokens() {
        let parsed = parse_env_vars(
            "MISSING_EQ =empty_key \"BAD KEY\"=1 OTHER\\ KEY=2 BAD\0KEY=3 KEY=bad\0val VALID=yes",
        );
        assert_eq!(
            parsed,
            BTreeMap::from([("VALID".to_string(), "yes".to_string())])
        );
    }

    #[test]
    fn test_format_env_vars_and_roundtrip() {
        let map = BTreeMap::from([
            ("A".to_string(), "1".to_string()),
            ("EMPTY".to_string(), "".to_string()),
            ("QUOTE".to_string(), r#"a"b"#.to_string()),
            ("SINGLE".to_string(), "it's".to_string()),
            ("SLASH".to_string(), r"C:\a\b".to_string()),
            ("SPACE".to_string(), "foo bar".to_string()),
        ]);
        assert_eq!(
            format_env_vars(&map),
            r#"A=1 EMPTY= QUOTE="a\"b" SINGLE="it's" SLASH="C:\\a\\b" SPACE="foo bar""#
        );

        let cases = vec![
            BTreeMap::new(),
            map,
            BTreeMap::from([
                ("OPT".to_string(), "x=y=z".to_string()),
                ("TAB_NEWLINE".to_string(), "a\tb\nc".to_string()),
                (
                    "MIXED".to_string(),
                    r#"hello 'single' "double" \end"#.to_string(),
                ),
            ]),
        ];

        for original in cases {
            let formatted = format_env_vars(&original);
            let parsed = parse_env_vars(&formatted);
            assert_eq!(
                original, parsed,
                "Env var roundtrip failed for formatted string: {formatted}"
            );
        }
    }
}

// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

/// Parses a single-line input JVM argument string into an argument list,
/// supporting single-quote and double-quote pairing as well as backslash escaping (similar to Shell lexical parsing)
pub fn parse_jvm_args(raw: &str) -> Vec<String> {
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

/// Formats a list of JVM arguments into a single-line string suitable for
/// shell-like lexical parsing via [`parse_jvm_args`].
pub fn format_jvm_args(args: &[String]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jvm_args_empty_and_whitespace() {
        assert_eq!(parse_jvm_args(""), Vec::<String>::new());
        assert_eq!(parse_jvm_args("   \t\n\r  "), Vec::<String>::new());
    }

    #[test]
    fn test_parse_jvm_args_basic() {
        assert_eq!(parse_jvm_args("-Xmx2G -Xms1G"), vec!["-Xmx2G", "-Xms1G"]);
        assert_eq!(
            parse_jvm_args("  -Xmx2G    -Xms1G  "),
            vec!["-Xmx2G", "-Xms1G"]
        );
    }

    #[test]
    fn test_parse_jvm_args_quotes_and_escapes() {
        assert_eq!(
            parse_jvm_args(r#"-Dfoo="bar baz" -Dtest='hello world'"#),
            vec!["-Dfoo=bar baz", "-Dtest=hello world"]
        );
        assert_eq!(
            parse_jvm_args(r#"-Dpath="C:\\Program Files\\App""#),
            vec![r"-Dpath=C:\Program Files\App"]
        );
        assert_eq!(
            parse_jvm_args(r#"-Dquote="\"escaped\"""#),
            vec![r#"-Dquote="escaped""#]
        );
        assert_eq!(parse_jvm_args(r"-Dfoo=bar\ baz"), vec!["-Dfoo=bar baz"]);
    }

    #[test]
    fn test_parse_jvm_args_unclosed_quotes_and_trailing_backslash() {
        assert_eq!(parse_jvm_args(r#"-Dfoo="bar"#), vec!["-Dfoo=bar"]);
        assert_eq!(parse_jvm_args(r"-Dfoo=bar\"), vec![r"-Dfoo=bar\"]);
        assert_eq!(parse_jvm_args(r#"-Dfoo="bar\"#), vec![r"-Dfoo=bar\"]);
    }

    #[test]
    fn test_format_jvm_args_basic() {
        assert_eq!(
            format_jvm_args(&["-Xmx2G".to_string(), "-Xms1G".to_string()]),
            "-Xmx2G -Xms1G"
        );
        assert_eq!(
            format_jvm_args(&[
                "-Dfoo=bar baz".to_string(),
                r"-Dpath=C:\test\app".to_string(),
                "".to_string(),
            ]),
            r#""-Dfoo=bar baz" "-Dpath=C:\\test\\app" """#
        );
    }

    #[test]
    fn test_jvm_args_roundtrip() {
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
            let formatted = format_jvm_args(&original);
            let parsed = parse_jvm_args(&formatted);
            assert_eq!(
                original, parsed,
                "Roundtrip failed for formatted string: {formatted}"
            );
        }
    }
}

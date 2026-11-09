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

pub fn format_jvm_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.contains(' ') || arg.contains('\t') || arg.contains('"') {
                let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

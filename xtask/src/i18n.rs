// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::bail;
use fluent_syntax::{
    ast::{Entry, Resource},
    serializer,
};
use proc_macro2::{TokenStream, TokenTree};
use serde::{Deserialize, Serialize};
use syn::{Expr, Fields, Item, LitStr, Meta, Token, parse::Parser, punctuated::Punctuated};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nConfig {
    pub fallback_language: String,
    pub available_languages: Vec<String>,
    pub fluent: FluentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FluentConfig {
    pub assets_dir: String,
}

impl I18nConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}

/// Convert "{name}" to the Fluent format "{ $name }".
pub fn to_fluent_pattern(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut var = String::new();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c == '}' {
                    break;
                }

                var.push(c);
            }
            let trimmed = var.trim();
            // Handle purely numeric variables in the tuple.
            if trimmed.chars().all(|c| c.is_ascii_digit()) {
                out.push_str(&format!("{{ $arg{trimmed} }}"));
            } else {
                out.push_str(&format!("{{ ${trimmed} }}"));
            }
        } else {
            out.push(ch);
        }
    }

    out
}

fn is_test_attr(group: &proc_macro2::Group) -> bool {
    if group.delimiter() != proc_macro2::Delimiter::Bracket {
        return false;
    }
    let s = group.stream().to_string().replace(' ', "");
    s == "test" || s == "cfg(test)" || s.ends_with("::test")
}

pub fn extract_t_from_stream(
    stream: TokenStream,
    entries: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let mut iter = stream.into_iter().peekable();
    while let Some(tree) = iter.next() {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == '#' => {
                if let Some(TokenTree::Group(group)) = iter.peek()
                    && is_test_attr(group)
                {
                    iter.next();
                    // Skip the test entire annotated item until its `{ ... }` body or `;`.
                    for next_tree in iter.by_ref() {
                        match next_tree {
                            TokenTree::Group(g)
                                if g.delimiter() == proc_macro2::Delimiter::Brace =>
                            {
                                break;
                            }
                            TokenTree::Punct(p) if p.as_char() == ';' => {
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
            TokenTree::Group(group) => {
                extract_t_from_stream(group.stream(), entries)?;
            }
            TokenTree::Ident(ident) if ident == "t" => {
                if let Some(TokenTree::Punct(punct)) = iter.peek()
                    && punct.as_char() == '!'
                {
                    iter.next(); // Consume the '!'.

                    if let Some(TokenTree::Group(group)) = iter.next() {
                        parse_t_args(group.stream(), entries)?;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn parse_t_args(tokens: TokenStream, entries: &mut BTreeMap<String, String>) -> anyhow::Result<()> {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let Ok(args) = parser.parse2(tokens) else {
        return Ok(()); // Skip the "macro_rules! t" definition body itself.
    };
    if args.len() < 2 {
        return Ok(());
    }
    let (Expr::Lit(key_expr), Expr::Lit(en_expr)) = (&args[0], &args[1]) else {
        return Ok(());
    };
    let (syn::Lit::Str(key_lit), syn::Lit::Str(en_lit)) = (&key_expr.lit, &en_expr.lit) else {
        return Ok(());
    };

    let key = key_lit.value();
    let fluent_val = to_fluent_pattern(&en_lit.value());

    if let Some(existing) = entries.get(&key)
        && existing != &fluent_val
    {
        bail!("Conflicting English text for key '{key}': '{existing}' vs '{fluent_val}'");
    }

    entries.insert(key, fluent_val);

    Ok(())
}

/// Convert CamelCase to kebab-case, e.g., "JavaNotFound" -> "java-not-found".
pub fn to_kebab_case(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                let prev_lower = chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit();
                let next_lower = i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase();

                if prev_lower || next_lower {
                    out.push('-');
                }
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }

    out
}

pub fn scan_gui_dir(dir: &Path, map: &mut BTreeMap<String, String>) -> anyhow::Result<()> {
    // Keep files ending in .rs except i18n_errors_gen.rs.
    let paths = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            name != "i18n_errors_gen.rs"
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|s| s.to_str() == Some("rs")));

    for path in paths {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let stream: TokenStream = content.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
        extract_t_from_stream(stream, map)?;
    }

    Ok(())
}

/// Handle the error messages in error enum.
/// Return generated "impl LocalizeError" code.
pub fn parse_error_enum(
    path: &Path,
    crate_name: &str,
    entries: &mut BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    let ast = syn::parse_file(&content)?;
    let mut out = String::new();

    for item in &ast.items {
        let Item::Enum(enum_item) = item else {
            continue;
        };

        let enum_ident = enum_item.ident.to_string();
        let enum_kebab = to_kebab_case(&enum_ident);
        let mut arms = Vec::new();

        for variant in &enum_item.variants {
            // Get the key and value.
            for attr in &variant.attrs {
                if !attr.path().is_ident("error") {
                    continue;
                }

                let Meta::List(list) = &attr.meta else {
                    continue;
                };

                let lit_str = list.parse_args::<LitStr>()?;
                let raw = lit_str.value();
                let v_ident = variant.ident.to_string();

                let key = format!("{enum_kebab}-{}", to_kebab_case(&v_ident));
                let value = to_fluent_pattern(&raw);

                if let Some(existing) = entries.get(&key)
                    && existing != &value
                {
                    bail!("Conflicting English text for key '{key}': '{existing}' vs '{value}'");
                }

                entries.insert(key.to_string(), value);

                let arm = match &variant.fields {
                    Fields::Unit => {
                        format!("            Self::{v_ident} => crate::t!({key:?}, {raw:?}),")
                    }
                    Fields::Unnamed(unnamed) => {
                        let mut en = raw.clone();
                        let mut pat_names = Vec::new();
                        let mut call_args = Vec::new();
                        for i in 0..unnamed.unnamed.len() {
                            en = en.replace(&format!("{{{i}}}"), &format!("{{arg{i}}}"));
                            pat_names.push(format!("arg{i}"));
                            call_args.push(format!("arg{i} = arg{i}"));
                        }
                        let pat = pat_names.join(", ");
                        let args = call_args.join(", ");
                        format!(
                            "            Self::{v_ident}({pat}) => crate::t!({key:?}, {en:?}, {args}),"
                        )
                    }
                    Fields::Named(named) => {
                        let names: Vec<String> = named
                            .named
                            .iter()
                            .filter_map(|f| f.ident.as_ref().map(ToString::to_string))
                            .collect();

                        let pat = names.join(", ");
                        let args = names
                            .iter()
                            .map(|n| format!("{n} = {n}"))
                            .collect::<Vec<_>>()
                            .join(", ");

                        format!(
                            "            Self::{v_ident} {{ {pat} }} => crate::t!({key:?}, {raw:?}, {args}),"
                        )
                    }
                };
                arms.push(arm);
            }
        }

        if !arms.is_empty() {
            let arms_str = arms.join("\n");
            out.push_str(&format!(
                 "\nimpl LocalizeError for {crate_name}::{enum_ident} {{\n    fn to_i18n(&self) -> String {{\n        match self {{\n{arms_str}\n        }}\n
                       }}\n}}\n"
            ));
        }
    }

    Ok(out)
}

/// Automatically generate "t!" calls for "thiserror" types
/// Provides "to_i18n" to obtain the already-translated text.
pub fn generate_all_errors(entries: &mut BTreeMap<String, String>) -> anyhow::Result<String> {
    let targets = [
        ("crates/acl-core/src/error.rs", "acl_core"),
        ("crates/acl-auth/src/error.rs", "acl_auth"),
        ("crates/acl-minecraft/src/error.rs", "acl_minecraft"),
    ];

    let mut code = String::from(
        "// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors\n\
         // SPDX-License-Identifier: GPL-3.0-or-later\n\
         //\n\
         // @generated by `cargo xtask i18n`. Do not edit manually.\n\n\
         use crate::i18n::LocalizeError;\n",
    );

    for (path, crate_name) in targets {
        let impl_block = parse_error_enum(Path::new(path), crate_name, entries)?;
        code.push_str(&impl_block);
    }

    Ok(code)
}

fn format_rust_code(code: &str) -> anyhow::Result<String> {
    let mut child = Command::new("rustfmt")
        .arg("--edition=2024")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(code.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("rustfmt failed");
    }
    Ok(String::from_utf8(output.stdout)?)
}

const FTL_HEADER: &str = "# SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors\n\
                          # SPDX-License-Identifier: GPL-3.0-or-later\n\n";

/// Build the FTL file content, automatically adding new keys and removing keys that no longer exist.
pub fn build_ftl_content(
    entries: &BTreeMap<String, String>,
    existing_ftl: Option<&str>,
) -> anyhow::Result<String> {
    let raw_en: String = entries
        .iter()
        .map(|(k, v)| format!("{k} = {v}\n"))
        .collect();
    let en_res = fluent_syntax::parser::parse(raw_en.as_str())
        .map_err(|(_, errs)| anyhow::anyhow!("Invalid generated Fluent entry: {errs:?}"))?;

    let existing_text = existing_ftl.unwrap_or_default();

    let mut existing_map = if existing_text.trim().is_empty() {
        BTreeMap::new()
    } else {
        let existing_res = fluent_syntax::parser::parse(existing_text)
            .map_err(|(_, errs)| anyhow::anyhow!("Fluent syntax error: {errs:?}"))?;
        let mut map = BTreeMap::new();
        for entry in existing_res.body {
            if let Entry::Message(msg) = entry {
                map.insert(msg.id.name, msg);
            }
        }
        map
    };

    let mut body = Vec::new();
    for entry in en_res.body {
        if let Entry::Message(en_msg) = entry {
            let msg = existing_map.remove(en_msg.id.name).unwrap_or(en_msg);
            body.push(Entry::Message(msg));
        }
    }

    let merged = Resource { body };
    Ok(format!("{FTL_HEADER}{}", serializer::serialize(&merged)))
}

fn write_or_check(path: &Path, content: &str, check: bool) -> anyhow::Result<()> {
    let current = fs::read_to_string(path).unwrap_or_default();
    if check {
        if current != content {
            bail!(
                "File '{}' is out of date. Please run `cargo xtask i18n`.",
                path.display()
            );
        }
        return Ok(());
    }

    if current != content {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, content)?;
    }

    Ok(())
}

pub fn run(check: bool) -> anyhow::Result<()> {
    let config_path = Path::new("crates/acl-gui/i18n.toml");
    let config = I18nConfig::load(config_path)?;

    let mut entries = BTreeMap::new();
    scan_gui_dir(Path::new("crates/acl-gui/src"), &mut entries)?;

    let raw_rs = generate_all_errors(&mut entries)?;
    let formatted_rs = format_rust_code(&raw_rs)?;
    write_or_check(
        Path::new("crates/acl-gui/src/i18n_errors_gen.rs"),
        &formatted_rs,
        check,
    )?;

    let assets_dir = Path::new("crates/acl-gui").join(&config.fluent.assets_dir);

    for lang in &config.available_languages {
        let ftl_path = assets_dir.join(lang).join("acl.ftl");
        let content = if lang == &config.fallback_language {
            build_ftl_content(&entries, None)
        } else {
            let old_text = fs::read_to_string(&ftl_path).unwrap_or_default();
            build_ftl_content(&entries, Some(&old_text))
        }?;
        write_or_check(&ftl_path, &content, check)?;
    }

    Ok(())
}

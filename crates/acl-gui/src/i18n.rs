// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::LazyLock;

use acl_common::Language;
use i18n_embed::{
    DesktopLanguageRequester, LanguageLoader,
    fluent::{FluentLanguageLoader, fluent_language_loader},
};
use rust_embed::RustEmbed;
use unic_langid::LanguageIdentifier;

#[derive(RustEmbed)]
#[folder = "../../assets/i18n/"]
struct Localizations;

pub static LANGUAGE_LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    if let Err(err) = loader.load_fallback_language(&Localizations) {
        tracing::error!("Failed to load fallback en-US language: {err}");
    }

    loader
});

pub fn apply_language(lang: Language) {
    let requested: Vec<LanguageIdentifier> = match lang.tag() {
        Some(tag) => tag.parse().into_iter().collect(),
        None => DesktopLanguageRequester::requested_languages(),
    };

    let _ = i18n_embed::select(&*LANGUAGE_LOADER, &Localizations, &requested);
}

// Garbage syntax
#[allow(unused_macros)]
#[macro_export]
macro_rules! t {
    ($message_id:literal, $default_en:literal) => {{
        $crate::i18n::LANGUAGE_LOADER.get($message_id)
    }};
    ($message_id:literal, $default_en:literal, $($key:ident = $value:expr),+$(,)?) => {{
        let mut args = std::collections::HashMap::new();

        $(
            args.insert(stringify!($key), $value.to_string());
        )+

        $crate::i18n::LANGUAGE_LOADER.get_args($message_id, args)

    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_loader_initialization_safety() {
        let loader = &*LANGUAGE_LOADER;
        let res = loader.get("nonexistent.key");
        assert!(!res.is_empty());
    }

    #[test]
    fn test_apply_language_all_variants() {
        let variants = [
            Language::System,
            Language::English,
            Language::SimplifiedChinese,
        ];

        for lang in variants {
            apply_language(lang);
        }
    }

    #[test]
    fn test_t_macro_basic_expansion() {
        let result = t!("app.name", "Adwaita Craft Launcher");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_t_macro_with_arguments() {
        let single_arg = t!("greet.user", "Hello {name}", name = "Player");
        assert!(!single_arg.is_empty());

        let multi_arg = t!("info.version", "{app} version {ver}", app = "ACL", ver = 1);
        assert!(!multi_arg.is_empty());
    }
}

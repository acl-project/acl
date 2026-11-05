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

pub trait LocalizeError {
    fn to_i18n(&self) -> String;
}

impl<T: LocalizeError + ?Sized> LocalizeError for &T {
    fn to_i18n(&self) -> String {
        (**self).to_i18n()
    }
}

// A hacky trick.

/// Perform type dispatch by reference, using autoref-based specialization
/// So that "LocalizeError" automatically calls "to_i18n()" to obtain the translated string
/// "Path" automatically uses "display()" to display the path
#[doc(hidden)]
pub struct I18nWrapper<T>(pub T);

#[doc(hidden)]
pub trait I18nErrorKind {
    fn to_i18n_arg(&self) -> String;
}
impl<T: LocalizeError> I18nErrorKind for &&I18nWrapper<T> {
    fn to_i18n_arg(&self) -> String {
        self.0.to_i18n()
    }
}

#[doc(hidden)]
pub trait I18nPathKind {
    fn to_i18n_arg(&self) -> String;
}
impl I18nPathKind for &I18nWrapper<&std::path::Path> {
    fn to_i18n_arg(&self) -> String {
        self.0.display().to_string()
    }
}
impl I18nPathKind for &I18nWrapper<&&std::path::Path> {
    fn to_i18n_arg(&self) -> String {
        self.0.display().to_string()
    }
}
impl I18nPathKind for &I18nWrapper<&std::path::PathBuf> {
    fn to_i18n_arg(&self) -> String {
        self.0.display().to_string()
    }
}
impl I18nPathKind for &I18nWrapper<&&std::path::PathBuf> {
    fn to_i18n_arg(&self) -> String {
        self.0.display().to_string()
    }
}

#[doc(hidden)]
pub trait I18nDisplayKind {
    fn to_i18n_arg(&self) -> String;
}
impl<T: std::fmt::Display> I18nDisplayKind for I18nWrapper<T> {
    fn to_i18n_arg(&self) -> String {
        self.0.to_string()
    }
}

#[macro_export]
macro_rules! t {
    // Garbage syntax

    // "default_en" is used by xtask to automatically generate the English source text.
    ($message_id:literal, $default_en:literal) => {{
        $crate::i18n::LANGUAGE_LOADER.get($message_id)
    }};
    ($message_id:literal, $default_en:literal, $($key:ident = $value:expr),+$(,)?) => {{
        let mut args = std::collections::HashMap::new();
        #[allow(unused_imports)]
        use $crate::i18n::{I18nDisplayKind as _, I18nErrorKind as _, I18nPathKind as _};
        $(
            args.insert(
                stringify!($key),
                (&&&$crate::i18n::I18nWrapper(&$value)).to_i18n_arg(),
            );
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

    #[test]
    fn test_localize_error_ref_blanket_impl() {
        struct MockError;
        impl LocalizeError for MockError {
            fn to_i18n(&self) -> String {
                "Mocked in-memory error".to_string()
            }
        }

        let err = MockError;
        let ref_err = &err;
        assert_eq!(ref_err.to_i18n(), "Mocked in-memory error");
        let ref_ref_err = &&err;
        assert_eq!(ref_ref_err.to_i18n(), "Mocked in-memory error");
    }

    #[test]
    #[allow(clippy::needless_borrow)]
    fn test_i18n_wrapper_path_dispatch() {
        #[allow(unused_imports)]
        use super::{I18nDisplayKind as _, I18nErrorKind as _, I18nPathKind as _};
        use std::path::{Path, PathBuf};

        let path = Path::new("/custom/mock/path.log");
        let arg1 = (&&&I18nWrapper(&path)).to_i18n_arg();
        assert_eq!(arg1, "/custom/mock/path.log");

        let path_buf = PathBuf::from("/custom/mock/path.log");
        let arg2 = (&&&I18nWrapper(&path_buf)).to_i18n_arg();
        assert_eq!(arg2, "/custom/mock/path.log");

        let arg3 = (&&&I18nWrapper(&&path_buf)).to_i18n_arg();
        assert_eq!(arg3, "/custom/mock/path.log");
    }

    #[test]
    #[allow(clippy::needless_borrow)]
    fn test_i18n_wrapper_error_kind_dispatch() {
        #[allow(unused_imports)]
        use super::{I18nDisplayKind as _, I18nErrorKind as _, I18nPathKind as _};

        struct MockError;
        impl LocalizeError for MockError {
            fn to_i18n(&self) -> String {
                "Mocked error message".to_string()
            }
        }

        let err = MockError;
        let arg1 = (&&&I18nWrapper(&err)).to_i18n_arg();
        assert_eq!(arg1, "Mocked error message");

        let ref_err = &err;
        let arg2 = (&&&I18nWrapper(&ref_err)).to_i18n_arg();
        assert_eq!(arg2, "Mocked error message");
    }

    #[test]
    #[allow(clippy::needless_borrow)]
    fn test_i18n_wrapper_display_fallback_dispatch() {
        #[allow(unused_imports)]
        use super::{I18nDisplayKind as _, I18nErrorKind as _, I18nPathKind as _};

        let val = 42;
        let arg1 = (&&&I18nWrapper(&val)).to_i18n_arg();
        assert_eq!(arg1, "42");

        let text = "sample_string";
        let arg2 = (&&&I18nWrapper(&text)).to_i18n_arg();
        assert_eq!(arg2, "sample_string");
    }
}

//! Reactive localization that composes with existing text and string-taking APIs.
//!
//! ```
//! cranpose_ui::run_test_composition(|| {
//!     let text = cranpose_ui::tr!("Hello, {name}!", name = "Ana");
//!     assert!(text.as_str().contains("Ana"));
//! });
//! ```
//!
//! Missing arguments are compile errors:
//!
//! ```compile_fail
//! let _ = cranpose_ui::tr!("Hello, {name}!");
//! ```
//!
//! Extra arguments are compile errors:
//!
//! ```compile_fail
//! let _ = cranpose_ui::tr!("Save", unused = 42);
//! ```

use std::cell::RefCell;

use cranpose_core::{
    CompositionLocal, CompositionLocalProvider, compositionLocalOf, remember, rememberKeyed,
};
pub use cranpose_localization::{
    Argument, Catalog, DeferredMessage, FluentValue, Language, LanguagePreference,
    LanguagePreferenceStore, Locale, Localization, LocalizationError, Message, PreviewMode,
    Resource, SourceCatalog, Translator,
};

/// A formatted translation with shared text storage and owned-string conversions.
pub type LocalizedText = crate::text::SharedText;

#[cfg(feature = "localization-formatting")]
pub use cranpose_localization::{FormatError, LocaleFormatters};

/// Translations for Cranpose controls, also usable by native services and test tools.
/// UI providers add this fallback automatically; applications can override its namespaces.
pub use crate::ui_strings::catalog as framework_catalog;

/// Locale-aware data formatters retained for the current language provider.
/// A language change replaces these formatters without changing stored application data.
#[cfg(feature = "localization-formatting")]
#[track_caller]
pub fn local_formatters() -> Result<std::rc::Rc<LocaleFormatters>, FormatError> {
    if let Some(translator) = local_translator().current() {
        return translator.formatters();
    }
    rememberKeyed((), |()| {
        LocaleFormatters::new(&Locale::parse("en").expect("source locale")).map(std::rc::Rc::new)
    })
}

/// Displays a message captured by `message!` using the current provider.
/// A language change updates the text even when the request remains in application state.
#[track_caller]
pub fn localized_message(message: &DeferredMessage) -> LocalizedText {
    let translator = local_translator().current();
    rememberKeyed((message.clone(), translator), |(message, translator)| {
        LocalizedText::from(message.format(translator.as_ref()).unwrap_or_else(|error| {
            log::error!("{error}");
            message.fallback_text()
        }))
    })
}

/// The translator installed by the nearest [`ProvideLocalization`].
/// `None` uses each message's inline source-language catalog.
pub fn local_translator() -> CompositionLocal<Option<Translator>> {
    crate::environment_locals::ENVIRONMENT_LOCALS.with(|locals| {
        locals
            .translator
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// Installs a catalog and language for this subtree, including its reading direction.
/// Change the supplied locale state to update text and accessibility descriptions.
/// Nested layout-direction providers can override the direction for individual controls.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideLocalization(catalog: &Catalog, locale: Locale, content: impl FnOnce()) {
    let translator = rememberKeyed((catalog.clone(), locale), |(catalog, locale)| {
        catalog
            .clone()
            .with_fallback(&crate::ui_strings::catalog())
            .translator(locale.clone())
    });
    provide(translator, content);
}

/// Installs a prepared translator, including one negotiated from several preferred languages.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideTranslator(translator: Translator, content: impl FnOnce()) {
    let translator = rememberKeyed(translator, |translator| {
        translator.with_fallback(&crate::ui_strings::catalog())
    });
    provide(translator, content);
}

#[track_caller]
fn provide(translator: Translator, content: impl FnOnce()) {
    let direction = if translator.locale().is_rtl() {
        crate::LayoutDirection::Rtl
    } else {
        crate::LayoutDirection::Ltr
    };
    let locales = rememberKeyed(translator.locale().clone(), |locale| {
        crate::text::LocaleList::new(vec![locale.to_string()])
    });
    CompositionLocalProvider(
        [
            local_translator().provides(Some(translator)),
            crate::local_layout_direction().provides(direction),
            text_locales().provides(Some(locales)),
        ],
        content,
    );
}

fn text_locales() -> CompositionLocal<Option<crate::text::LocaleList>> {
    crate::environment_locals::ENVIRONMENT_LOCALS.with(|locals| {
        locals
            .text_locales
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

pub(crate) fn apply_text_locale(mut style: crate::text::TextStyle) -> crate::text::TextStyle {
    if style.span_style.locale_list.is_none() {
        style.span_style.locale_list = text_locales().current();
    }
    style
}

struct CachedTranslation {
    translator: Option<Translator>,
    message: &'static Message,
    arguments: Vec<Argument<'static>>,
    text: LocalizedText,
}

/// Resolves a static message at this composition position. Prefer [`crate::tr!`].
/// Unchanged arguments, catalog, and language reuse the previous formatted text.
#[track_caller]
pub fn localized<const N: usize>(
    message: &'static Message,
    mut arguments: [Argument<'_>; N],
) -> LocalizedText {
    localized_impl(message, &mut arguments)
}

#[track_caller]
fn localized_impl(message: &'static Message, arguments: &mut [Argument<'_>]) -> LocalizedText {
    let translator = local_translator().current();
    let slot = remember(|| RefCell::new(None::<CachedTranslation>));
    slot.with(|slot| {
        let mut cached = slot.borrow_mut();
        if let Some(value) = &*cached
            && value.translator == translator
            && std::ptr::eq(value.message, message)
            && value.arguments.as_slice() == &*arguments
        {
            return value.text.clone();
        }
        let formatted = match &translator {
            Some(translator) => translator.format(message, arguments),
            None => message.format_source(arguments),
        };
        let text = LocalizedText::from(formatted.unwrap_or_else(|error| {
            log::error!("{error}");
            message.fallback_text(arguments)
        }));
        if let Some(value) = &mut *cached {
            value.translator = translator;
            value.message = message;
            value.text = text.clone();
            for (index, argument) in arguments.iter_mut().enumerate() {
                match value.arguments.get_mut(index) {
                    Some(previous) => previous.update_from(argument),
                    None => value.arguments.push(argument.take_owned()),
                }
            }
            value.arguments.truncate(arguments.len());
        } else {
            *cached = Some(CachedTranslation {
                translator,
                message,
                arguments: arguments.iter_mut().map(Argument::take_owned).collect(),
                text: text.clone(),
            });
        }
        text
    })
}

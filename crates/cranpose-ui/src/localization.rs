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

use std::cell::{OnceCell, RefCell};

use cranpose_core::{
    CompositionLocal, CompositionLocalProvider, compositionLocalOf, remember, rememberKeyed,
};
pub use cranpose_localization::{
    Argument, Catalog, FluentValue, Locale, LocalizationError, Message, PreviewMode, Resource,
    SourceCatalog, Translator,
};

/// A formatted translation with shared text storage and owned-string conversions.
pub type LocalizedText = crate::text::SharedText;

/// The translator installed by the nearest [`ProvideLocalization`].
/// `None` uses each message's inline source-language catalog.
pub fn local_translator() -> CompositionLocal<Option<Translator>> {
    thread_local! {
        static LOCAL: OnceCell<CompositionLocal<Option<Translator>>> = const { OnceCell::new() };
    }
    LOCAL.with(|local| local.get_or_init(|| compositionLocalOf(|| None)).clone())
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
    CompositionLocalProvider(
        [
            local_translator().provides(Some(translator)),
            crate::local_layout_direction().provides(direction),
        ],
        content,
    );
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

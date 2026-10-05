use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::OnceLock};

use fluent_bundle::{FluentArgs, FluentBundle, FluentError, FluentResource, FluentValue};

use crate::LocalizationError;

/// A static message definition emitted by `tr!` or generated catalog bindings.
pub struct Message {
    namespace: &'static str,
    id: &'static str,
    source: &'static SourceCatalog,
}

/// Source-language messages shared by generated accessors and parsed on first use.
pub struct SourceCatalog {
    source_locale: &'static str,
    resource: &'static str,
    parsed: OnceLock<Result<FluentResource, String>>,
}

impl Message {
    /// Defines a message using a source catalog containing `id`.
    /// Prefer the macros, which validate the source at compile time.
    pub const fn new(
        namespace: &'static str,
        id: &'static str,
        source: &'static SourceCatalog,
    ) -> Self {
        Self {
            namespace,
            id,
            source,
        }
    }

    /// The package namespace used for catalog lookup and app overrides.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// The stable catalog identifier.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Formats the source-language message without any application catalog.
    pub fn format_source(&self, arguments: &[Argument<'_>]) -> Result<String, LocalizationError> {
        self.source(&fluent_args(arguments))
    }

    /// Returns best-effort source text after a formatting error, preserving readable
    /// text and Fluent's unresolved-variable markers instead of showing a catalog key.
    pub fn fallback_text(&self, arguments: &[Argument<'_>]) -> String {
        self.source_output(&fluent_args(arguments))
            .map_or_else(|_| "…".to_owned(), |(text, _)| text)
    }

    pub(crate) fn source(&self, args: &FluentArgs<'_>) -> Result<String, LocalizationError> {
        let (text, errors) = self.source_output(args)?;
        if errors.is_empty() {
            Ok(text)
        } else {
            Err(self.error(format!("{errors:?}")))
        }
    }

    fn source_output(
        &self,
        args: &FluentArgs<'_>,
    ) -> Result<(String, Vec<FluentError>), LocalizationError> {
        let bundle = self.source.bundle().map_err(|detail| self.error(detail))?;
        let pattern = bundle
            .get_message(self.id)
            .and_then(|message| message.value())
            .ok_or_else(|| self.error("source message has no value".to_owned()))?;
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, Some(args), &mut errors);
        Ok((text.into_owned(), errors))
    }

    pub(crate) fn error(&self, detail: String) -> LocalizationError {
        LocalizationError::Format {
            namespace: self.namespace.to_owned(),
            id: self.id.to_owned(),
            detail,
        }
    }
}

pub(crate) fn fluent_args<'a>(arguments: &'a [Argument<'_>]) -> FluentArgs<'a> {
    arguments
        .iter()
        .map(|argument| {
            let value = match &argument.value {
                FluentValue::String(text) => FluentValue::from(text.as_ref()),
                value => value.clone(),
            };
            (argument.name, value)
        })
        .collect()
}

impl SourceCatalog {
    /// Defines source-language Fluent data. Macros validate it before compilation.
    pub const fn new(source_locale: &'static str, resource: &'static str) -> Self {
        Self {
            source_locale,
            resource,
            parsed: OnceLock::new(),
        }
    }

    fn bundle(&'static self) -> Result<Rc<FluentBundle<&'static FluentResource>>, String> {
        type SourceBundle = Result<Rc<FluentBundle<&'static FluentResource>>, String>;
        thread_local! {
            static SOURCES: RefCell<BTreeMap<usize, SourceBundle>> = const { RefCell::new(BTreeMap::new()) };
        }
        SOURCES.with(|sources| {
            sources
                .borrow_mut()
                .entry(std::ptr::from_ref(self).addr())
                .or_insert_with(|| self.prepare().map(Rc::new))
                .clone()
        })
    }

    fn prepare(&'static self) -> Result<FluentBundle<&'static FluentResource>, String> {
        let language = self
            .source_locale
            .parse()
            .map_err(|error| format!("{error}"))?;
        let resource = self
            .parsed
            .get_or_init(|| {
                FluentResource::try_new(self.resource.to_owned())
                    .map_err(|(_, errors)| format!("{errors:?}"))
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let mut bundle = crate::new_bundle(language);
        bundle
            .add_resource(resource)
            .map_err(|errors| format!("{errors:?}"))?;
        Ok(bundle)
    }
}

/// A named value passed to a message without allocating a string for numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Argument<'a> {
    pub(crate) name: &'static str,
    pub(crate) value: FluentValue<'a>,
}

impl<'a> Argument<'a> {
    /// Borrows strings and preserves numeric values for Fluent plural selection.
    pub fn new(name: &'static str, value: impl Into<FluentValue<'a>>) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }

    /// Transfers owned values into a cache, copying only borrowed text that must outlive the call.
    pub fn take_owned(&mut self) -> Argument<'static> {
        let value = match std::mem::replace(&mut self.value, FluentValue::None) {
            FluentValue::String(text) => FluentValue::String(text.into_owned().into()),
            FluentValue::Number(number) => FluentValue::Number(number),
            FluentValue::Custom(custom) => FluentValue::Custom(custom),
            FluentValue::None => FluentValue::None,
            FluentValue::Error => FluentValue::Error,
        };
        Argument {
            name: self.name,
            value,
        }
    }

    /// Updates an owned cache argument, reusing string storage when possible.
    pub fn update_from(&mut self, source: &mut Argument<'_>) {
        self.name = source.name;
        if let FluentValue::String(target) = &mut self.value
            && let FluentValue::String(std::borrow::Cow::Borrowed(value)) = &source.value
        {
            let target = target.to_mut();
            target.clear();
            target.push_str(value);
            return;
        }
        self.value = source.take_owned().value;
    }
}

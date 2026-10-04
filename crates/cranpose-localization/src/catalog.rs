use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use fluent_syntax::ast::Entry;
use unic_langid::LanguageIdentifier;

use crate::{
    Argument, Locale, LocalizationError, Message, locale::preview_text, message::fluent_args,
};

type Resources = BTreeMap<LanguageIdentifier, BTreeMap<Arc<str>, Vec<Arc<FluentResource>>>>;
type Prepared = BTreeMap<Arc<str>, Vec<FluentBundle<Arc<FluentResource>>>>;

/// One embedded or application-loaded Fluent resource.
#[derive(Clone, Copy)]
pub struct Resource<'a> {
    /// Language tag, for example `fr-CA`.
    pub locale: &'a str,
    /// Owning package, or the package being overridden by the application.
    pub namespace: &'a str,
    /// Complete UTF-8 Fluent source.
    pub source: &'a str,
}

struct CatalogData {
    fallback: Locale,
    languages: Vec<LanguageIdentifier>,
    resources: Resources,
    parents: Vec<Catalog>,
}

/// Immutable, shareable parsed catalogs. Replacing a catalog replaces its cache identity.
#[derive(Clone)]
pub struct Catalog(Arc<CatalogData>);

impl PartialEq for Catalog {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Catalog {
    /// Parses catalogs once, rejecting malformed resources and duplicate definitions.
    /// Missing translations fall back to `fallback`, then the message's inline source.
    pub fn from_resources(
        fallback: &str,
        resources: &[Resource<'_>],
    ) -> Result<Self, LocalizationError> {
        let fallback = Locale::parse(fallback)?;
        let mut parsed: Resources = BTreeMap::new();
        for resource in resources {
            let language = Locale::parse(resource.locale)?.language;
            let source = FluentResource::try_new(resource.source.to_owned())
                .map_err(|(_, errors)| resource_error(resource, format!("{errors:?}")))?;
            parsed
                .entry(language)
                .or_default()
                .entry(Arc::from(resource.namespace))
                .or_default()
                .push(Arc::new(source));
        }
        validate_duplicates(&parsed)?;
        let languages = parsed.keys().cloned().collect();
        Ok(Self(Arc::new(CatalogData {
            fallback,
            languages,
            resources: parsed,
            parents: Vec::new(),
        })))
    }

    /// Adds a library's catalogs below application overrides without copying its resources.
    /// Lookup prefers an application message in the same language before the library's.
    pub fn with_fallback(self, library: &Catalog) -> Self {
        let mut languages = self.0.languages.clone();
        languages.extend(library.0.languages.iter().cloned());
        languages.sort();
        languages.dedup();
        Self(Arc::new(CatalogData {
            fallback: self.0.fallback.clone(),
            languages,
            resources: BTreeMap::new(),
            parents: vec![self, library.clone()],
        }))
    }

    fn prepare(&self, language: &LanguageIdentifier, output: &mut Prepared) {
        if let Some(namespaces) = self.0.resources.get(language) {
            for (namespace, sources) in namespaces {
                let mut bundle = crate::new_bundle(language.clone());
                for resource in sources {
                    bundle
                        .add_resource(Arc::clone(resource))
                        .expect("catalog rejects duplicate entries");
                }
                output
                    .entry(Arc::clone(namespace))
                    .or_default()
                    .push(bundle);
            }
        }
        for parent in &self.0.parents {
            parent.prepare(language, output);
        }
    }

    /// Creates a thread-owned formatter for an explicit language.
    pub fn translator(&self, locale: Locale) -> Translator {
        self.negotiate(std::slice::from_ref(&locale))
    }

    /// Negotiates ordered user preferences, preserving script subtags.
    /// An empty list selects the catalog's source language. The resulting formatter
    /// stays on the calling thread; pass a cloned catalog to background workers.
    pub fn negotiate(&self, preferences: &[Locale]) -> Translator {
        let requested: Vec<_> = preferences.iter().map(|locale| &locale.language).collect();
        let languages = negotiate_languages(
            &requested,
            &self.0.languages,
            Some(&self.0.fallback.language),
            NegotiationStrategy::Filtering,
        );
        let prepared = languages
            .iter()
            .map(|language| {
                let mut prepared = Prepared::new();
                self.prepare(language, &mut prepared);
                prepared
            })
            .collect();
        let locale = Locale {
            language: languages.first().map_or_else(
                || self.0.fallback.language.clone(),
                |language| (*language).clone(),
            ),
            preview: preferences.first().map_or_default(|locale| locale.preview),
        };
        Translator(Rc::new(TranslatorData {
            catalog: self.clone(),
            locale,
            prepared,
            preferences: preferences.to_vec(),
        }))
    }

    /// The source language used when no requested translation is available.
    pub fn fallback_locale(&self) -> &Locale {
        &self.0.fallback
    }
}

struct TranslatorData {
    catalog: Catalog,
    locale: Locale,
    prepared: Vec<Prepared>,
    preferences: Vec<Locale>,
}

/// A thread-owned formatter with no cross-thread locks during formatting.
/// Construct one from a shared [`Catalog`] on each thread that needs translations.
#[derive(Clone)]
pub struct Translator(Rc<TranslatorData>);

impl PartialEq for Translator {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.catalog == other.0.catalog && self.0.preferences == other.0.preferences)
    }
}

impl Translator {
    /// The first selected catalog language and the requested preview settings.
    /// Layout direction follows this effective language, including on fallback.
    pub fn locale(&self) -> &Locale {
        &self.0.locale
    }

    /// Adds library translations while retaining the original language preferences.
    pub fn with_fallback(&self, library: &Catalog) -> Self {
        self.0
            .catalog
            .clone()
            .with_fallback(library)
            .negotiate(&self.0.preferences)
    }

    /// Formats a message, trying negotiated catalogs and then its source.
    /// Broken translations are skipped; a broken source returns an error.
    pub fn format(
        &self,
        message: &Message,
        arguments: &[Argument<'_>],
    ) -> Result<String, LocalizationError> {
        let args = fluent_args(arguments);
        let text = self
            .translated(message, &args)
            .map_or_else(|| message.source(&args), Ok)?;
        Ok(preview_text(text, self.0.locale.preview))
    }

    fn translated(&self, message: &Message, args: &FluentArgs<'_>) -> Option<String> {
        for prepared in &self.0.prepared {
            let Some(bundles) = prepared.get(message.namespace()) else {
                continue;
            };
            for bundle in bundles {
                let Some(pattern) = bundle.get_message(message.id()).and_then(|msg| msg.value())
                else {
                    continue;
                };
                let mut errors = Vec::new();
                let text = bundle.format_pattern(pattern, Some(args), &mut errors);
                if errors.is_empty() {
                    return Some(text.into_owned());
                }
            }
        }
        None
    }
}

fn validate_duplicates(resources: &Resources) -> Result<(), LocalizationError> {
    for (language, namespaces) in resources {
        for (namespace, sources) in namespaces {
            let mut identifiers = BTreeSet::from(["NUMBER"]);
            for source in sources {
                for entry in source.entries() {
                    let key = match entry {
                        Entry::Message(message) => message.id.name,
                        Entry::Term(term) => term.id.name,
                        _ => continue,
                    };
                    if !identifiers.insert(key) {
                        return Err(LocalizationError::InvalidResource {
                            resource: format!("{language}/{namespace}"),
                            detail: format!("duplicate or reserved entry `{key}`"),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn resource_error(resource: &Resource<'_>, detail: String) -> LocalizationError {
    LocalizationError::InvalidResource {
        resource: format!("{}/{}", resource.locale, resource.namespace),
        detail,
    }
}

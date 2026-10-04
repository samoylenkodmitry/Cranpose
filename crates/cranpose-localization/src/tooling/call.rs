use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Expr, Ident, LitStr, Token,
    parse::{Parse, ParseStream},
};

use super::catalog_signatures;

/// A checked `tr!` invocation, shared by compilation and source extraction.
pub struct TranslationCall {
    /// Source-language pattern after lowering simple named placeholders to Fluent.
    pub pattern: String,
    /// Stable explicit or source-derived message identifier.
    pub id: String,
    /// Translator-facing disambiguation.
    pub context: Option<String>,
    /// Guidance for translators.
    pub comment: Option<String>,
    /// Source-language tag used to format the inline fallback.
    pub source_locale: String,
    /// Named Rust argument expressions.
    pub arguments: Vec<(Ident, Expr)>,
    span: Span,
}

impl Parse for TranslationCall {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let source: LitStr = input.parse()?;
        let mut call = Self {
            pattern: normalize(&source.value()),
            id: String::new(),
            context: None,
            comment: None,
            source_locale: String::new(),
            arguments: Vec::new(),
            span: source.span(),
        };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            call.parse_option(input)?;
        }
        if call.id.is_empty() {
            call.id = source_id(&call.pattern, call.context.as_deref());
        }
        if call.source_locale.is_empty() {
            call.source_locale.push_str("en");
        }
        call.source_locale = crate::Locale::parse(&call.source_locale)
            .map_err(|error| syn::Error::new(call.span, error))?
            .to_string();
        call.validate()?;
        Ok(call)
    }
}

impl TranslationCall {
    fn parse_option(&mut self, input: ParseStream<'_>) -> syn::Result<()> {
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        match name.to_string().as_str() {
            "id" => set_once(&mut self.id, input.parse::<LitStr>()?.value(), name.span())?,
            "context" => set_option(
                &mut self.context,
                input.parse::<LitStr>()?.value(),
                name.span(),
            )?,
            "comment" => set_option(
                &mut self.comment,
                input.parse::<LitStr>()?.value(),
                name.span(),
            )?,
            "source_locale" => set_once(
                &mut self.source_locale,
                input.parse::<LitStr>()?.value(),
                name.span(),
            )?,
            _ => self.arguments.push((name, input.parse()?)),
        }
        Ok(())
    }

    /// A complete Fluent entry, suitable for extraction and the source fallback.
    pub fn resource(&self) -> String {
        let pattern = self.pattern.replace('\n', "\n    ");
        format!("{} = {}\n", self.id, pattern)
    }

    /// Expands into a static message and a reactive lookup through `ui_path`.
    pub fn expand(&self, ui_path: &TokenStream) -> TokenStream {
        let id = &self.id;
        let source_locale = &self.source_locale;
        let resource = self.resource();
        let arguments = self.arguments.iter().map(|(name, expression)| {
            let key = name.to_string();
            quote!(#ui_path::localization::Argument::new(#key, #expression))
        });
        quote!({
            static SOURCE: #ui_path::localization::SourceCatalog =
                #ui_path::localization::SourceCatalog::new(#source_locale, #resource);
            static MESSAGE: #ui_path::localization::Message =
                #ui_path::localization::Message::new(env!("CARGO_PKG_NAME"), #id, &SOURCE);
            #ui_path::localization::localized(&MESSAGE, [#(#arguments),*])
        })
    }

    fn validate(&self) -> syn::Result<()> {
        let signatures = catalog_signatures(&self.resource())
            .map_err(|error| syn::Error::new(self.span, error))?;
        let signature = signatures
            .get(&self.id)
            .ok_or_else(|| syn::Error::new(self.span, "message must have a value"))?;
        let mut provided = BTreeSet::new();
        for (name, _) in &self.arguments {
            if !provided.insert(name.to_string()) {
                return Err(syn::Error::new(name.span(), "duplicate argument"));
            }
        }
        if signature.variables != provided {
            return Err(syn::Error::new(
                self.span,
                format!(
                    "message needs arguments {:?}, provided {:?}",
                    signature.variables, provided,
                ),
            ));
        }
        Ok(())
    }
}

fn set_once(slot: &mut String, value: String, span: Span) -> syn::Result<()> {
    if value.is_empty() {
        return Err(syn::Error::new(span, "option must not be empty"));
    }
    if !slot.is_empty() {
        return Err(syn::Error::new(span, "duplicate option"));
    }
    *slot = value;
    Ok(())
}

fn set_option(slot: &mut Option<String>, value: String, span: Span) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        return Err(syn::Error::new(span, "duplicate option"));
    }
    Ok(())
}

fn source_id(pattern: &str, context: Option<&str>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in context
        .unwrap_or_default()
        .bytes()
        .chain([0])
        .chain(pattern.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("m-{hash:016x}")
}

fn normalize(source: &str) -> String {
    let mut normalized = String::with_capacity(source.len());
    let mut remaining = source;
    while let Some(start) = remaining.find('{') {
        normalized.push_str(&remaining[..start]);
        remaining = &remaining[start + 1..];
        let Some(end) = remaining.find('}') else {
            normalized.push('{');
            break;
        };
        let placeholder = &remaining[..end];
        let named = placeholder.starts_with(|ch: char| ch.is_ascii_alphabetic())
            && placeholder
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        if named {
            normalized.push_str("{ $");
            normalized.push_str(placeholder);
            normalized.push_str(" }");
            remaining = &remaining[end + 1..];
        } else {
            normalized.push('{');
        }
    }
    normalized.push_str(remaining);
    normalized
}

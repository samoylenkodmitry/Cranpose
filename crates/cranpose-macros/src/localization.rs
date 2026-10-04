use std::{env, fs, path::PathBuf};

use cranpose_localization::tooling::{
    TranslationCall, catalog_signatures, load_catalogs, validate_catalogs,
};
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Ident, LitStr, Token, Visibility,
    parse::{Parse, ParseStream},
};

fn ui_path() -> syn::Result<TokenStream> {
    for package in ["cranpose-ui", "cranpose"] {
        if let Ok(found) = crate_name(package) {
            return Ok(match found {
                FoundCrate::Itself => {
                    let name = format_ident!("{}", package.replace('-', "_"));
                    quote!(::#name)
                }
                FoundCrate::Name(name) => {
                    let name = format_ident!("{}", name);
                    quote!(::#name)
                }
            });
        }
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "enable localization on cranpose or cranpose-ui",
    ))
}

pub(crate) fn translate(input: TokenStream) -> syn::Result<TokenStream> {
    let call: TranslationCall = syn::parse2(input)?;
    Ok(call.expand(&ui_path()?))
}

struct CatalogInput {
    directory: LitStr,
    fallback: LitStr,
}

impl Parse for CatalogInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let directory = input.parse()?;
        input.parse::<Token![,]>()?;
        let option: Ident = input.parse()?;
        if option != "fallback" {
            return Err(syn::Error::new(option.span(), "expected fallback"));
        }
        input.parse::<Token![=]>()?;
        let fallback = input.parse()?;
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            directory,
            fallback,
        })
    }
}

fn absolute_path(path: &LitStr) -> syn::Result<PathBuf> {
    let root = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| syn::Error::new(path.span(), "CARGO_MANIFEST_DIR is unavailable"))?;
    Ok(PathBuf::from(root).join(path.value()))
}

pub(crate) fn catalogs(input: TokenStream) -> syn::Result<TokenStream> {
    let input: CatalogInput = syn::parse2(input)?;
    let files = load_catalogs(&absolute_path(&input.directory)?)
        .map_err(|error| syn::Error::new(input.directory.span(), error))?;
    validate_catalogs(&files, &input.fallback.value())
        .map_err(|error| syn::Error::new(input.directory.span(), error))?;
    let path = ui_path()?;
    let fallback = input.fallback;
    let resources = files.iter().map(|file| {
        let locale = &file.locale;
        let namespace = &file.namespace;
        let filename = file.path.to_string_lossy();
        quote!(#path::localization::Resource { locale: #locale, namespace: #namespace, source: include_str!(#filename) })
    });
    Ok(quote!({
        static CATALOG: ::std::sync::LazyLock<#path::localization::Catalog> = ::std::sync::LazyLock::new(|| {
            #path::localization::Catalog::from_resources(#fallback, &[#(#resources),*])
                .expect("catalogs validated by translations!")
        });
        CATALOG.clone()
    }))
}

struct BindingsInput {
    visibility: Visibility,
    name: Ident,
    file: LitStr,
}

impl Parse for BindingsInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let visibility = input.parse()?;
        input.parse::<Token![mod]>()?;
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let file = input.parse()?;
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            visibility,
            name,
            file,
        })
    }
}

pub(crate) fn bindings(input: TokenStream) -> syn::Result<TokenStream> {
    let input: BindingsInput = syn::parse2(input)?;
    let filename = absolute_path(&input.file)?;
    let source =
        fs::read_to_string(&filename).map_err(|error| syn::Error::new(input.file.span(), error))?;
    let signatures =
        catalog_signatures(&source).map_err(|error| syn::Error::new(input.file.span(), error))?;
    let locale = filename
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .ok_or_else(|| syn::Error::new(input.file.span(), "expected <locale>/<namespace>.ftl"))?;
    cranpose_localization::Locale::parse(locale)
        .map_err(|error| syn::Error::new(input.file.span(), error))?;
    let namespace = filename
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| syn::Error::new(input.file.span(), "missing namespace"))?;
    let path = ui_path()?;
    let functions = accessors(signatures, namespace, &path, input.file.span())?;
    let visibility = input.visibility;
    let name = input.name;
    let filename = filename.to_string_lossy();
    Ok(quote!(
        #[doc = "Generated localized message accessors."]
        #visibility mod #name {
            static SOURCE: #path::localization::SourceCatalog = #path::localization::SourceCatalog::new(#locale, include_str!(#filename));
            #(#functions)*
        }
    ))
}

fn accessors(
    signatures: std::collections::BTreeMap<
        String,
        cranpose_localization::tooling::MessageSignature,
    >,
    namespace: &str,
    path: &TokenStream,
    span: proc_macro2::Span,
) -> syn::Result<Vec<TokenStream>> {
    let mut functions = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (id, signature) in signatures {
        let name = rust_name(&id, span)?;
        if !names.insert(name.to_string()) {
            return Err(syn::Error::new(
                span,
                "message names collide after replacing hyphens",
            ));
        }
        let arguments: Vec<_> = signature
            .variables
            .iter()
            .map(|name| rust_name(name, span))
            .collect::<Result<_, _>>()?;
        let keys = signature.variables.iter();
        let generics = if arguments.is_empty() {
            quote!()
        } else {
            quote!(<'a>)
        };
        let doc = if signature.comment.is_empty() {
            format!("Localizes `{namespace}/{id}`.")
        } else {
            signature.comment
        };
        functions.push(quote!(
            #[doc = #doc]
            #[track_caller]
            pub fn #name #generics (#(#arguments: impl Into<#path::localization::FluentValue<'a>>),*) -> #path::localization::LocalizedText {
                static MESSAGE: #path::localization::Message = #path::localization::Message::new(#namespace, #id, &SOURCE);
                #path::localization::localized(&MESSAGE, [#(#path::localization::Argument::new(#keys, #arguments)),*])
            }
        ));
    }
    Ok(functions)
}

fn rust_name(name: &str, span: proc_macro2::Span) -> syn::Result<Ident> {
    let name = name.replace('-', "_");
    syn::parse_str::<Ident>(&name)
        .or_else(|_| syn::parse_str::<Ident>(&format!("r#{name}")))
        .map_err(|_| {
            syn::Error::new(
                span,
                "message or argument cannot be represented as a Rust identifier",
            )
        })
}

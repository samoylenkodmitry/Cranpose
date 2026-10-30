#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use packed_params::PackedParams;
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use returning_call::ReturningCall;
use syn::{FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type, parse_macro_input};

mod branch_groups;
mod constant_modifiers;
#[cfg(feature = "localization")]
mod localization;
mod packed_params;
mod preview;
mod returning_call;

/// Localizes readable source text using the nearest localization provider.
/// Named arguments use `name = expression`; optional literal metadata is
/// `id`, `context`, `comment`, and `source_locale` (default `en`).
#[cfg(feature = "localization")]
#[proc_macro]
pub fn tr(input: TokenStream) -> TokenStream {
    localization::translate(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Embeds `<directory>/<locale>/<namespace>.ftl` catalogs and checks them at compile time.
/// Syntax: `translations!("locales", fallback = "en")`.
#[cfg(feature = "localization")]
#[proc_macro]
pub fn translations(input: TokenStream) -> TokenStream {
    localization::catalogs(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Captures readable source text and arguments for translation when displayed.
/// Accepts the same syntax as `tr!`, but works outside composition.
#[cfg(feature = "localization")]
#[proc_macro]
pub fn message(input: TokenStream) -> TokenStream {
    localization::message(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Generates a module of named message accessors from a source-language Fluent file.
/// Syntax: `translation_messages!(pub mod messages, "locales/en/app.ftl")`.
#[cfg(feature = "localization")]
#[proc_macro]
pub fn translation_messages(input: TokenStream) -> TokenStream {
    localization::bindings(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}
#[cfg(test)]
#[path = "tests/preview_tests.rs"]
mod preview_tests;

/// Registers a parameterless component fixture for IDE previews.
///
/// Enable Cranpose's `preview` feature. Options are `name`, `group`, `width`,
/// `height`, and `dark`. Combine this attribute with `#[composable]`.
#[proc_macro_attribute]
pub fn preview(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    preview::expand(args.into(), function)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

fn is_fn_like_type(ty: &Type) -> bool {
    match ty {
        Type::ImplTrait(impl_trait) => impl_trait.bounds.iter().any(|bound| {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                let path = &trait_bound.path;
                if let Some(segment) = path.segments.last() {
                    let ident_str = segment.ident.to_string();
                    return ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce";
                }
            }
            false
        }),
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last()
                && segment.ident == "Box"
                && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                && let Some(syn::GenericArgument::Type(Type::TraitObject(trait_obj))) =
                    args.args.first()
            {
                return trait_obj.bounds.iter().any(|bound| {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound {
                        let path = &trait_bound.path;
                        if let Some(segment) = path.segments.last() {
                            let ident_str = segment.ident.to_string();
                            return ident_str == "FnMut"
                                || ident_str == "Fn"
                                || ident_str == "FnOnce";
                        }
                    }
                    false
                });
            }
            false
        }
        Type::FnPtr(_) => true,
        _ => false,
    }
}

fn is_generic_fn_like(ty: &Type, generics: &syn::Generics) -> bool {
    let type_ident = match ty {
        Type::Path(type_path) if type_path.path.segments.len() == 1 => {
            &type_path.path.segments[0].ident
        }
        _ => return false,
    };

    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param
            && type_param.ident == *type_ident
        {
            for bound in &type_param.bounds {
                if let syn::TypeParamBound::Trait(trait_bound) = bound
                    && let Some(segment) = trait_bound.path.segments.last()
                {
                    let ident_str = segment.ident.to_string();
                    if ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce" {
                        return true;
                    }
                }
            }
        }
    }

    if let Some(where_clause) = &generics.where_clause {
        for predicate in &where_clause.predicates {
            if let syn::WherePredicate::Type(pred) = predicate
                && let Type::Path(bounded_type) = &pred.bounded_ty
                && bounded_type.path.segments.len() == 1
                && bounded_type.path.segments[0].ident == *type_ident
            {
                for bound in &pred.bounds {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound
                        && let Some(segment) = trait_bound.path.segments.last()
                    {
                        let ident_str = segment.ident.to_string();
                        if ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce" {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// A parameter of a `#[composable]` function.
struct ParamInfo {
    ident: Ident,
    pat: Box<Pat>,
    ty: Type,
    pat_is_mut: bool,
    is_impl_trait: bool,
}

/// Whether a call can skip its body: a callback or `impl Trait` parameter
/// marks every call changed.
fn every_param_compared(param_info: &[ParamInfo], param_is_callback: &[bool]) -> bool {
    param_info
        .iter()
        .zip(param_is_callback)
        .all(|(info, is_callback)| !*is_callback && !info.is_impl_trait)
}

fn is_fn_param(ty: &Type, generics: &syn::Generics) -> bool {
    is_fn_like_type(ty) || is_generic_fn_like(ty, generics)
}

fn is_zero_arg_fn_impl_trait(ty: &Type) -> bool {
    if let Type::ImplTrait(impl_trait) = ty {
        impl_trait.bounds.iter().any(|bound| {
            if let syn::TypeParamBound::Trait(trait_bound) = bound
                && let Some(segment) = trait_bound.path.segments.last()
            {
                let ident_str = segment.ident.to_string();
                if (ident_str == "Fn" || ident_str == "FnMut")
                    && let syn::PathArguments::Parenthesized(args) = &segment.arguments
                {
                    let returns_unit = match &args.output {
                        ReturnType::Default => true,
                        ReturnType::Type(_, ty) => {
                            matches!(&**ty, Type::Tuple(tuple) if tuple.elems.is_empty())
                        }
                    };
                    return args.inputs.is_empty() && returns_unit;
                }
            }
            false
        })
    } else {
        false
    }
}

fn type_bare_generic_ident(ty: &Type) -> Option<&Ident> {
    match ty {
        Type::Path(type_path)
            if type_path.qself.is_none()
                && type_path.path.segments.len() == 1
                && type_path.path.segments[0].arguments.is_none() =>
        {
            Some(&type_path.path.segments[0].ident)
        }
        _ => None,
    }
}

fn stream_mentions_ident(tokens: &TokenStream2, name: &str) -> bool {
    tokens.clone().into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(ident) => ident == name,
        proc_macro2::TokenTree::Group(group) => stream_mentions_ident(&group.stream(), name),
        _ => false,
    })
}

fn filter_generics(
    generics: &syn::Generics,
    strip: &std::collections::HashSet<String>,
) -> syn::Generics {
    let mut filtered = generics.clone();
    filtered.params = filtered
        .params
        .into_iter()
        .filter(|param| match param {
            syn::GenericParam::Type(type_param) => !strip.contains(&type_param.ident.to_string()),
            _ => true,
        })
        .collect();
    if let Some(where_clause) = &mut filtered.where_clause {
        where_clause.predicates = where_clause
            .predicates
            .clone()
            .into_iter()
            .filter(|predicate| {
                if let syn::WherePredicate::Type(pred) = predicate
                    && let Some(ident) = type_bare_generic_ident(&pred.bounded_ty)
                {
                    return !strip.contains(&ident.to_string());
                }
                true
            })
            .collect();
        if where_clause.predicates.is_empty() {
            filtered.where_clause = None;
        }
    }
    filtered
}

fn is_node_id_return(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Path(type_path)
            if type_path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "NodeId")
    )
}

fn core_crate_path() -> TokenStream2 {
    let crate_name = crate_name("cranpose")
        .ok()
        .or_else(|| crate_name("cranpose-core").ok());

    match crate_name {
        Some(FoundCrate::Itself) => quote!(crate),
        Some(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(#ident)
        }
        None => quote!(cranpose_core),
    }
}

/// The key of a slot the expansion takes: a constant of the call site, or
/// under hot reload, where a patch moves call sites, the caller's location
/// as a call computes it.
fn slot_key(core_path: &TokenStream2) -> TokenStream2 {
    if cfg!(feature = "hot-reload") {
        return quote! { #core_path::caller_location_key() };
    }
    quote! {{
        const __CRANPOSE_SLOT_KEY: #core_path::Key =
            #core_path::const_location_key(file!(), line!(), column!());
        #core_path::noted_location_key(__CRANPOSE_SLOT_KEY, file!(), line!(), column!())
    }}
}

fn definition_key_stmt(
    core_path: &TokenStream2,
    caller_key_ident: &Ident,
    name: &Ident,
) -> TokenStream2 {
    if cfg!(feature = "hot-reload") {
        // A hot patch moves definitions and may reuse a static cache for a
        // different item. Development keys name the definition instead.
        let name = name.to_string();
        return quote! {
            let #caller_key_ident = #core_path::composable_identity_key({
                const __CRANPOSE_DEFINITION_KEY: #core_path::Key =
                    #core_path::hot_definition_key(file!(), module_path!(), #name);
                __CRANPOSE_DEFINITION_KEY
            });
        };
    }
    quote! {
        let #caller_key_ident = #core_path::composable_identity_key({
            struct __CranposeDefinitionMarker;
            static __CRANPOSE_DEFINITION_KEY: ::std::sync::OnceLock<#core_path::Key> =
                ::std::sync::OnceLock::new();
            #core_path::cached_composable_definition_key(
                &__CRANPOSE_DEFINITION_KEY,
                file!(),
                line!(),
                column!(),
                ::std::any::TypeId::of::<__CranposeDefinitionMarker>(),
            )
        });
    }
}

/// Turns a function into a composable: its body runs inside a group keyed by
/// the call site, and recomposition skips it while its arguments are
/// unchanged. `#[composable(no_skip)]` always re-runs the body and stores no
/// parameters: as Compose's inline composables, it composes in its caller's
/// group, where what it composes is matched by position, as a loop's items
/// are, so a state it or its content reads recomposes the composable that
/// called it.
///
/// Composables are named in CamelCase, as in Jetpack Compose, so the
/// generated function carries `#[allow(non_snake_case)]` and callers need no
/// lint allowance of their own.
#[proc_macro_attribute]
pub fn composable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_tokens = TokenStream2::from(attr);
    let mut enable_skip = true;
    let core_path = core_crate_path();
    if !attr_tokens.is_empty() {
        match syn::parse2::<Ident>(attr_tokens) {
            Ok(ident) if ident == "no_skip" => enable_skip = false,
            Ok(other) => {
                return syn::Error::new_spanned(other, "unsupported composable attribute")
                    .to_compile_error()
                    .into();
            }
            Err(err) => {
                return err.to_compile_error().into();
            }
        }
    }

    let mut func = parse_macro_input!(item as ItemFn);

    let mut param_info: Vec<ParamInfo> = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            if let Some(reserved) = find_reserved_pattern_ident(pat) {
                let name = reserved.to_string();
                return syn::Error::new(
                    reserved.span(),
                    format!("`{name}` is reserved by #[composable]"),
                )
                .to_compile_error()
                .into();
            }
            let pat_is_mut = matches!(
                pat.as_ref(),
                Pat::Ident(pat_ident) if pat_ident.mutability.is_some()
            );
            let is_impl_trait = matches!(**ty, Type::ImplTrait(_));

            if is_impl_trait {
                let ident = if let Pat::Ident(pat_ident) = &**pat {
                    pat_ident.ident.clone()
                } else {
                    Ident::new(&format!("__arg{index}"), Span::mixed_site())
                };
                param_info.push(ParamInfo {
                    ident,
                    pat: pat.clone(),
                    ty: ty.as_ref().clone(),
                    pat_is_mut,
                    is_impl_trait: true,
                });
            } else {
                let ident = Ident::new(&format!("__arg{index}"), Span::mixed_site());
                let original_pat: Box<Pat> = pat.clone();
                **pat = syn::parse_quote! { #ident };
                param_info.push(ParamInfo {
                    ident,
                    pat: original_pat,
                    ty: ty.as_ref().clone(),
                    pat_is_mut,
                    is_impl_trait: false,
                });
            }
        }
    }

    inject_groups(&core_path, &mut func);
    let has_rust_abi = match &func.sig.abi {
        None => true,
        Some(abi) => abi.name.as_ref().is_some_and(|name| name.value() == "Rust"),
    };
    if has_rust_abi {
        func.attrs.push(syn::parse_quote!(#[track_caller]));
    }
    func.attrs.push(syn::parse_quote!(
        #[allow(non_snake_case, reason = "names follow Compose's casing")]
    ));

    let scope_label_ident = func.sig.ident.clone();
    let original_block = func.block.clone();
    let body_end = func.block.brace_token.span.close();
    let composer_ident = Ident::new("__composer", Span::mixed_site());
    let slot_key = slot_key(&core_path);
    let caller_key_ident = Ident::new("__cranpose_caller_key", Span::mixed_site());
    let current_scope_ident = Ident::new("__current_scope", Span::mixed_site());
    let result_ident = Ident::new("__result", Span::mixed_site());
    let group_ident = Ident::new("__cranpose_group", Span::mixed_site());
    let key_expr = quote! { #caller_key_ident };
    let caller_key_stmt = definition_key_stmt(&core_path, &caller_key_ident, &scope_label_ident);

    let rebinds_for_no_skip: Vec<_> = param_info
        .iter()
        .map(|info| {
            let ident = &info.ident;
            let pat = &info.pat;
            quote! { let #pat = #ident; }
        })
        .collect();

    let return_ty: Type = match &func.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };
    let returns_unit = returns_unit(&func.sig.output);
    let invalidate_return_consumer = if returns_unit || is_node_id_return(&return_ty) {
        quote! {}
    } else {
        quote! { #composer_ident.__invalidate_return_consumer_scope(); }
    };
    let _helper_ident = Ident::new(
        &format!("__cranpose_impl_{}", func.sig.ident),
        Span::mixed_site(),
    );
    let generics = func.sig.generics.clone();
    let (_impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();

    let _helper_inputs: Vec<TokenStream2> = param_info
        .iter()
        .map(|info| {
            let ident = &info.ident;
            let ty = &info.ty;
            quote! { #ident: #ty }
        })
        .collect();

    let has_unhandled_impl_trait = param_info
        .iter()
        .any(|info| info.is_impl_trait && !is_zero_arg_fn_impl_trait(&info.ty));

    if enable_skip && !has_unhandled_impl_trait {
        let helper_ident = Ident::new(
            &format!("__cranpose_impl_{}", func.sig.ident),
            Span::mixed_site(),
        );
        let generics = func.sig.generics.clone();

        let param_erased: Vec<bool> = param_info
            .iter()
            .map(|info| {
                (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait
                        && type_bare_generic_ident(&info.ty).is_some()
                        && is_generic_fn_like(&info.ty, &generics))
            })
            .collect();

        let mut strippable: std::collections::HashSet<String> = param_info
            .iter()
            .zip(&param_erased)
            .filter(|(info, erased)| **erased && !info.is_impl_trait)
            .filter_map(|(info, _)| type_bare_generic_ident(&info.ty))
            .map(Ident::to_string)
            .collect();
        loop {
            use quote::ToTokens;
            let mut used_elsewhere: Vec<TokenStream2> = Vec::new();
            for (info, erased) in param_info.iter().zip(&param_erased) {
                if !*erased {
                    used_elsewhere.push(info.ty.to_token_stream());
                }
            }
            used_elsewhere.push(return_ty.to_token_stream());
            for param in &generics.params {
                match param {
                    syn::GenericParam::Type(type_param) => {
                        if !strippable.contains(&type_param.ident.to_string()) {
                            used_elsewhere.push(type_param.bounds.to_token_stream());
                            if let Some((_, default)) = &type_param.default {
                                used_elsewhere.push(default.to_token_stream());
                            }
                        }
                    }
                    syn::GenericParam::Const(const_param) => {
                        used_elsewhere.push(const_param.ty.to_token_stream());
                    }
                    syn::GenericParam::Lifetime(_) => {}
                }
            }
            if let Some(where_clause) = &generics.where_clause {
                for predicate in &where_clause.predicates {
                    if let syn::WherePredicate::Type(pred) = predicate
                        && let Some(ident) = type_bare_generic_ident(&pred.bounded_ty)
                        && strippable.contains(&ident.to_string())
                    {
                        continue;
                    }
                    used_elsewhere.push(predicate.to_token_stream());
                }
            }
            let before = strippable.len();
            strippable.retain(|name| {
                !used_elsewhere
                    .iter()
                    .any(|tokens| stream_mentions_ident(tokens, name))
            });
            if strippable.len() == before {
                break;
            }
        }

        let helper_generics = filter_generics(&generics, &strippable);
        let (impl_generics, ty_generics, where_clause) = helper_generics.split_for_impl();
        let ty_generics_turbofish = ty_generics.as_turbofish();

        let helper_inputs: Vec<TokenStream2> = param_info
            .iter()
            .zip(&param_erased)
            .filter_map(|(info, erased)| {
                if info.is_impl_trait && !is_zero_arg_fn_impl_trait(&info.ty) {
                    None
                } else if *erased {
                    let ident = &info.ident;
                    Some(quote! { #ident: ::std::boxed::Box<dyn ::core::ops::FnMut() + 'static> })
                } else {
                    let ident = &info.ident;
                    let ty = &info.ty;
                    Some(quote! { #ident: #ty })
                }
            })
            .collect();

        let param_state_slots: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_state_slot{index}"), Span::mixed_site()))
            .collect();

        let param_is_callback: Vec<bool> = param_info
            .iter()
            .map(|info| {
                (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait && is_fn_param(&info.ty, &generics))
            })
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .zip(&param_erased)
            .zip(&param_is_callback)
            .map(|(((info, slot_ident), erased), is_callback)| {
                if *is_callback {
                    let ident = &info.ident;
                    let update = if *erased {
                        quote! { holder.update_boxed(#ident); }
                    } else {
                        quote! { holder.update(#ident); }
                    };
                    quote! {
                        let (#slot_ident, ()) = #composer_ident.__update_param_slot(
                            #slot_key,
                            || #core_path::CallbackHolder::new(),
                            |holder: &mut #core_path::CallbackHolder| {
                                #update
                            },
                        );
                        __changed = true;
                    }
                } else if info.is_impl_trait {
                    quote! { __changed = true; }
                } else {
                    quote! {}
                }
            })
            .collect();

        let param_setup_recompose: Vec<TokenStream2> = param_state_slots
            .iter()
            .zip(&param_is_callback)
            .filter(|(_, is_callback)| **is_callback)
            .map(|(slot_ident, _)| {
                quote! {
                    let #slot_ident = #composer_ident
                        .__use_param_slot(#slot_key, || #core_path::CallbackHolder::new());
                }
            })
            .collect();

        let callback_rebinds: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .zip(&param_is_callback)
            .filter(|(_, is_callback)| **is_callback)
            .map(|((info, slot_ident), _)| {
                let pat = &info.pat;
                let can_add_mut = matches!(pat.as_ref(), Pat::Ident(_));
                let binding = if can_add_mut && !info.pat_is_mut {
                    quote! { mut #pat }
                } else {
                    quote! { #pat }
                };
                quote! {
                    #[allow(unused_mut, reason = "only some callbacks change the binding")]
                    let #binding = #composer_ident
                        .with_slot_value::<#core_path::CallbackHolder, _>(
                            #slot_ident,
                            |holder| holder.clone_rc(),
                        );
                }
            })
            .collect();

        let body_inputs: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .zip(&param_is_callback)
            .map(|((info, slot_ident), is_callback)| {
                if *is_callback {
                    quote! {
                        #slot_ident: #core_path::ValueSlotHandle<'_, #core_path::CallbackHolder>
                    }
                } else {
                    let pat = &info.pat;
                    let ty = &info.ty;
                    quote! { #pat: #ty }
                }
            })
            .collect();

        let body_args: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .zip(&param_is_callback)
            .map(|((info, slot_ident), is_callback)| {
                if *is_callback {
                    quote! { #slot_ident }
                } else {
                    let ident = &info.ident;
                    quote! { #ident }
                }
            })
            .collect();

        let packed = PackedParams {
            params: param_info
                .iter()
                .zip(&param_is_callback)
                .filter(|(info, is_callback)| !**is_callback && !info.is_impl_trait)
                .map(|(info, _)| (&info.ident, &info.ty))
                .collect(),
            returns: (!returns_unit).then_some(&return_ty),
        };
        let skippable = every_param_compared(&param_info, &param_is_callback);
        let param_setup = packed.with_setup(
            &core_path,
            &composer_ident,
            (&ReturningCall::previous(), skippable),
            param_setup,
        );
        let param_setup_recompose =
            packed.with_slot(&core_path, &composer_ident, param_setup_recompose);
        let reads_for_recompose = packed.reads(&core_path, &composer_ident);

        let body_ident = Ident::new(
            &format!("__cranpose_body_{}", func.sig.ident),
            Span::mixed_site(),
        );
        let body_call = quote! {
            #body_ident #ty_generics_turbofish (#composer_ident #(, #body_args)*)
        };

        let recompose_fn_ident = Ident::new(
            &format!("__cranpose_recompose_{}", func.sig.ident),
            Span::mixed_site(),
        );

        let recompose_setter = recompose_setter(
            &group_ident,
            &quote! { #recompose_fn_ident #ty_generics_turbofish },
            &func.sig.output,
            (&core_path, &composer_ident),
        );

        let returning = ReturningCall {
            core_path: &core_path,
            composer: &composer_ident,
            group: &group_ident,
            current_scope: &current_scope_ident,
            scope_label: &scope_label_ident,
            return_ty: &return_ty,
            slot_key: &slot_key,
            packed: &packed,
            skippable,
            body_call: &body_call,
        };
        let helper_body = if returns_unit {
            quote! {
                #core_path::debug_label_current_scope(stringify!(#scope_label_ident));
                let mut __changed = #current_scope_ident.should_recompose();
                #(#param_setup)*
                #recompose_setter
                if !__changed && #current_scope_ident.has_composed_once() {
                    #group_ident.skip();
                    return;
                }
                #body_call
            }
        } else {
            returning.helper_body(&param_setup, &recompose_setter)
        };

        let recompose_fn_body = if returns_unit {
            quote! {
                #(#param_setup_recompose)*
                #(#reads_for_recompose)*
                #body_call;
            }
        } else {
            returning.recompose_body(
                &param_setup_recompose,
                &reads_for_recompose,
                &invalidate_return_consumer,
            )
        };

        let slot_origin = hot_slot_origin(&core_path, &func.sig.ident, body_end);
        let recompose_fn = quote! {
            #[allow(non_snake_case, reason = "names follow Compose's casing")]
            fn #recompose_fn_ident #impl_generics (
                #composer_ident: &#core_path::Composer
            ) -> #return_ty #where_clause {
                #slot_origin
                #recompose_fn_body
            }
        };

        let helper_fn = quote! {
            #[allow(non_snake_case, clippy::too_many_arguments, reason = "the helper takes the composable's own parameters")]
            fn #helper_ident #impl_generics (
                #composer_ident: &#core_path::Composer,
                #group_ident: &#core_path::ComposableGroup<'_>
                #(, #helper_inputs)*
            ) -> #return_ty #where_clause {
                #slot_origin
                let #current_scope_ident = #group_ident.scope();
                #helper_body
            }
        };

        let body_fn = quote! {
            #[allow(non_snake_case, clippy::too_many_arguments, reason = "the helper takes the composable's own parameters")]
            fn #body_ident #impl_generics (
                #composer_ident: &#core_path::Composer
                #(, #body_inputs)*
            ) -> #return_ty #where_clause {
                #(#callback_rebinds)*
                let _cranpose_source_scope = #core_path::__source_scope(
                    stringify!(#scope_label_ident), file!(), line!(), env!("CARGO_MANIFEST_DIR"),
                );
                #original_block
            }
        };

        let wrapper_args: Vec<TokenStream2> = param_info
            .iter()
            .zip(&param_erased)
            .filter_map(|(info, erased)| {
                if info.is_impl_trait && !is_zero_arg_fn_impl_trait(&info.ty) {
                    None
                } else if *erased {
                    let ident = &info.ident;
                    Some(quote! { ::std::boxed::Box::new(#ident) })
                } else {
                    let ident = &info.ident;
                    Some(quote! { #ident })
                }
            })
            .collect();

        let wrapped = quote!({
            #caller_key_stmt
            let #composer_ident = #core_path::__current_composer();
            let #group_ident = #composer_ident.__open_composable_group(#key_expr);
            let #result_ident = #helper_ident(
                &#composer_ident,
                &#group_ident
                #(, #wrapper_args)*
            );
            #group_ident.close();
            #result_ident
        });
        *func.block = syn::parse2(wrapped).expect("failed to build block");
        TokenStream::from(quote! {
            #body_fn
            #recompose_fn
            #helper_fn
            #func
        })
    } else {
        let wrapped = quote!({
            #core_path::with_current_composer(|#composer_ident: &#core_path::Composer| {
                #(#rebinds_for_no_skip)*
                let _cranpose_source_scope = #core_path::__source_scope(
                    stringify!(#scope_label_ident), file!(), line!(), env!("CARGO_MANIFEST_DIR"),
                );
                #original_block
            })
        });
        *func.block = syn::parse2(wrapped).expect("failed to build block");
        TokenStream::from(quote! { #func })
    }
}

/// Adds branch groups; with the development-only `hot-reload` feature the
/// body also marks its definition as the origin for call-site keys.
fn inject_groups(core_path: &TokenStream2, func: &mut ItemFn) {
    let hot_reload = cfg!(feature = "hot-reload");
    branch_groups::inject_branch_groups_with(
        core_path,
        &mut func.block,
        &func.sig.ident,
        hot_reload,
    );
    // A hot patch keeps the slots of unchanged code, so a kept chain whose
    // constants it edits would stay as it was.
    if !hot_reload {
        constant_modifiers::keep_constant_modifiers(core_path, &mut func.block);
    }
    if hot_reload {
        let end = func.block.brace_token.span.close();
        let origin = branch_groups::hot_origin_stmt(core_path, &func.sig.ident, end);
        func.block
            .stmts
            .insert(0, syn::parse2(origin).expect("hot origin statement"));
    }
}

// Whether a function returns `()`.
fn returns_unit(output: &ReturnType) -> bool {
    match output {
        ReturnType::Default => true,
        ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty())
        }
    }
}

// The statement that gives a composable's group its recompose function: a
// plain function needs no box, vtable or call shim per composable, and one
// that returns a value gets a wrapper that drops it.
fn recompose_setter(
    group: &Ident,
    recompose_fn: &TokenStream2,
    output: &ReturnType,
    (core_path, composer): (&TokenStream2, &Ident),
) -> TokenStream2 {
    if returns_unit(output) {
        quote! { #group.set_recompose_fn(#recompose_fn); }
    } else {
        quote! {
            #group.set_recompose_fn(|#composer: &#core_path::Composer| {
                let _ = #recompose_fn(#composer);
            });
        }
    }
}

/// Parameter and return slots are keyed by call sites in generated code. With
/// hot reload those keys follow the current origin, so the first composition
/// and an independent recomposition both set the composable's own origin.
fn hot_slot_origin(core_path: &TokenStream2, name: &Ident, end: Span) -> TokenStream2 {
    if cfg!(feature = "hot-reload") {
        branch_groups::hot_origin_stmt(core_path, name, end)
    } else {
        TokenStream2::new()
    }
}

fn find_reserved_pattern_ident(pat: &Pat) -> Option<&Ident> {
    use syn::visit::Visit;

    struct Scan<'ast> {
        found: Option<&'ast Ident>,
    }
    impl<'ast> Visit<'ast> for Scan<'ast> {
        fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
            if self.found.is_none() {
                let name = node.ident.to_string();
                if name == "__composer" || name.starts_with("__cranpose") {
                    self.found = Some(&node.ident);
                }
            }
            syn::visit::visit_pat_ident(self, node);
        }
    }
    let mut scan = Scan { found: None };
    scan.visit_pat(pat);
    scan.found
}

#[cfg(test)]
#[path = "tests/macros_tests.rs"]
mod tests;

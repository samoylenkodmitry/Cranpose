use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type};

mod branch_groups;

/// Check if a type is Fn-like (impl FnMut/Fn/FnOnce, Box<dyn FnMut>, generic with Fn bound, etc.)
/// For generic type parameters (e.g., `F` where F: FnMut()), we need to check the bounds.
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
            if let Some(segment) = type_path.path.segments.last() {
                if segment.ident == "Box" {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(Type::TraitObject(trait_obj))) =
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
                    }
                }
            }
            false
        }
        Type::BareFn(_) => true,
        _ => false,
    }
}

/// Check if a generic type parameter has Fn-like bounds by looking at the where clause and bounds
fn is_generic_fn_like(ty: &Type, generics: &syn::Generics) -> bool {
    let type_ident = match ty {
        Type::Path(type_path) if type_path.path.segments.len() == 1 => {
            &type_path.path.segments[0].ident
        }
        _ => return false,
    };

    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param {
            if type_param.ident == *type_ident {
                for bound in &type_param.bounds {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound {
                        if let Some(segment) = trait_bound.path.segments.last() {
                            let ident_str = segment.ident.to_string();
                            if ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce" {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(where_clause) = &generics.where_clause {
        for predicate in &where_clause.predicates {
            if let syn::WherePredicate::Type(pred) = predicate {
                if let Type::Path(bounded_type) = &pred.bounded_ty {
                    if bounded_type.path.segments.len() == 1
                        && bounded_type.path.segments[0].ident == *type_ident
                    {
                        for bound in &pred.bounds {
                            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                                if let Some(segment) = trait_bound.path.segments.last() {
                                    let ident_str = segment.ident.to_string();
                                    if ident_str == "FnMut"
                                        || ident_str == "Fn"
                                        || ident_str == "FnOnce"
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Unified check: is this type Fn-like, either syntactically or via generic bounds?
fn is_fn_param(ty: &Type, generics: &syn::Generics) -> bool {
    is_fn_like_type(ty) || is_generic_fn_like(ty, generics)
}

/// Check if a type is `impl Fn() + ...` or `impl FnMut() + ...` with **zero** arguments.
/// Only these can be stored through [`CallbackHolder`] (excludes `FnOnce` which can't be
/// called more than once).
fn is_zero_arg_fn_impl_trait(ty: &Type) -> bool {
    if let Type::ImplTrait(impl_trait) = ty {
        impl_trait.bounds.iter().any(|bound| {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                if let Some(segment) = trait_bound.path.segments.last() {
                    let ident_str = segment.ident.to_string();
                    if ident_str == "Fn" || ident_str == "FnMut" {
                        if let syn::PathArguments::Parenthesized(args) = &segment.arguments {
                            return args.inputs.is_empty();
                        }
                    }
                }
            }
            false
        })
    } else {
        false
    }
}

/// The bare ident of a plain single-segment type path (e.g. the `F` in
/// `content: F`), if that is what the type is.
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

/// Whether the token stream mentions an ident with this exact name anywhere.
fn stream_mentions_ident(tokens: &TokenStream2, name: &str) -> bool {
    tokens.clone().into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(ident) => ident == name,
        proc_macro2::TokenTree::Group(group) => stream_mentions_ident(&group.stream(), name),
        _ => false,
    })
}

/// Clone `generics` without the stripped type parameters and without the
/// where-clause predicates that constrain them.
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
                if let syn::WherePredicate::Type(pred) = predicate {
                    if let Some(ident) = type_bare_generic_ident(&pred.bounded_ty) {
                        return !strip.contains(&ident.to_string());
                    }
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

    struct ParamInfo {
        ident: Ident,
        pat: Box<Pat>,
        ty: Type,
        pat_is_mut: bool,
        is_impl_trait: bool,
    }

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
                let original_pat: Box<Pat> = pat.clone();
                if let Pat::Ident(pat_ident) = &**pat {
                    param_info.push(ParamInfo {
                        ident: pat_ident.ident.clone(),
                        pat: original_pat,
                        ty: ty.as_ref().clone(),
                        pat_is_mut,
                        is_impl_trait: true,
                    });
                } else {
                    param_info.push(ParamInfo {
                        ident: Ident::new(&format!("__arg{}", index), Span::call_site()),
                        pat: original_pat,
                        ty: ty.as_ref().clone(),
                        pat_is_mut,
                        is_impl_trait: true,
                    });
                }
            } else {
                let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
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

    branch_groups::inject_branch_groups(&core_path, &mut func.block);
    func.attrs.push(syn::parse_quote!(#[track_caller]));

    let scope_label_ident = func.sig.ident.clone();
    let original_block = func.block.clone();
    let helper_block = original_block.clone();
    let recompose_block = original_block.clone();
    let key_expr = quote! { __cranpose_caller_key };
    let caller_key_stmt = quote! {
        let __cranpose_caller_key = #core_path::composable_identity_key({
            static __CRANPOSE_DEFINITION_KEY: ::std::sync::OnceLock<#core_path::Key> =
                ::std::sync::OnceLock::new();
            *__CRANPOSE_DEFINITION_KEY
                .get_or_init(|| #core_path::location_key(file!(), line!(), column!()))
        });
    };

    let rebinds_for_no_skip: Vec<_> = param_info
        .iter()
        .map(|info| {
            let ident = &info.ident;
            let pat = &info.pat;
            quote! { let #pat = #ident; }
        })
        .collect();

    let return_ty: syn::Type = match &func.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };
    let returns_unit = match &func.sig.output {
        ReturnType::Default => true,
        ReturnType::Type(_, ty) => {
            matches!(ty.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty())
        }
    };
    let invalidate_return_consumer = if returns_unit || is_node_id_return(&return_ty) {
        quote! {}
    } else {
        quote! { __composer.__invalidate_return_consumer_scope(); }
    };
    let _helper_ident = Ident::new(
        &format!("__cranpose_impl_{}", func.sig.ident),
        Span::call_site(),
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
            Span::call_site(),
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
                            if let Some(default) = &type_param.default {
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
                    if let syn::WherePredicate::Type(pred) = predicate {
                        if let Some(ident) = type_bare_generic_ident(&pred.bounded_ty) {
                            if strippable.contains(&ident.to_string()) {
                                continue;
                            }
                        }
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
            .map(|index| Ident::new(&format!("__param_state_slot{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .zip(&param_erased)
            .map(|((info, slot_ident), erased)| {
                if (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait && is_fn_param(&info.ty, &generics))
                {
                    let ident = &info.ident;
                    let update = if *erased {
                        quote! { holder.update_boxed(#ident); }
                    } else {
                        quote! { holder.update(#ident); }
                    };
                    quote! {
                        let #slot_ident = __composer
                            .__use_param_slot(|| #core_path::CallbackHolder::new());
                        __composer.with_slot_value::<#core_path::CallbackHolder, _>(
                            #slot_ident,
                            |holder| {
                                #update
                            },
                        );
                        __changed = true;
                    }
                } else if info.is_impl_trait {
                    quote! { __changed = true; }
                } else {
                    let ident = &info.ident;
                    let ty = &info.ty;
                    quote! {
                        let #slot_ident = __composer
                            .__use_param_slot(|| #core_path::ParamState::<#ty>::default());
                        if __composer.with_slot_value_mut::<#core_path::ParamState<#ty>, _>(
                            #slot_ident,
                            |state| state.update(&#ident),
                        )
                        {
                            __changed = true;
                        }
                    }
                }
            })
            .collect();

        let param_setup_recompose: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .map(|(info, slot_ident)| {
                if (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait && is_fn_param(&info.ty, &generics))
                {
                    quote! {
                        let #slot_ident = __composer
                            .__use_param_slot(|| #core_path::CallbackHolder::new());
                    }
                } else if info.is_impl_trait {
                    quote! {}
                } else {
                    let ty = &info.ty;
                    quote! {
                        let #slot_ident = __composer
                            .__use_param_slot(|| #core_path::ParamState::<#ty>::default());
                    }
                }
            })
            .collect();

        let rebinds: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .map(|(info, slot_ident)| {
                if (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait && is_fn_param(&info.ty, &generics))
                {
                    let pat = &info.pat;
                    let can_add_mut = matches!(pat.as_ref(), Pat::Ident(_));
                    if can_add_mut && !info.pat_is_mut {
                        quote! {
                            #[allow(unused_mut)]
                            let mut #pat = __composer
                                .with_slot_value::<#core_path::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    } else {
                        quote! {
                            #[allow(unused_mut)]
                            let #pat = __composer
                                .with_slot_value::<#core_path::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    }
                } else if info.is_impl_trait {
                    quote! {}
                } else {
                    let pat = &info.pat;
                    let ident = &info.ident;
                    quote! {
                        let #pat = #ident;
                    }
                }
            })
            .collect();

        let rebinds_for_recompose: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .map(|(info, slot_ident)| {
                if (info.is_impl_trait && is_zero_arg_fn_impl_trait(&info.ty))
                    || (!info.is_impl_trait && is_fn_param(&info.ty, &generics))
                {
                    let pat = &info.pat;
                    let can_add_mut = matches!(pat.as_ref(), Pat::Ident(_));
                    if can_add_mut && !info.pat_is_mut {
                        quote! {
                            #[allow(unused_mut)]
                            let mut #pat = __composer
                                .with_slot_value::<#core_path::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    } else {
                        quote! {
                            #[allow(unused_mut)]
                            let #pat = __composer
                                .with_slot_value::<#core_path::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    }
                } else if info.is_impl_trait {
                    quote! {}
                } else {
                    let pat = &info.pat;
                    let ty = &info.ty;
                    quote! {
                        let #pat = __composer
                            .with_slot_value::<#core_path::ParamState<#ty>, _>(
                                #slot_ident,
                                |state| {
                                    state
                                        .value()
                                        .expect("composable parameter missing for recomposition")
                                },
                            );
                    }
                }
            })
            .collect();

        let recompose_fn_ident = Ident::new(
            &format!("__cranpose_recompose_{}", func.sig.ident),
            Span::call_site(),
        );

        let recompose_setter = quote! {
            {
                __composer.set_recompose_callback(move |
                    __composer: &#core_path::Composer|
                {
                    let _ = #recompose_fn_ident #ty_generics_turbofish (
                        __composer
                    );
                });
            }
        };

        let helper_body = if returns_unit {
            quote! {
                #core_path::debug_label_current_scope(stringify!(#scope_label_ident));
                let __current_scope = __composer
                    .current_recompose_scope()
                    .expect("missing recompose scope");
                let mut __changed = __current_scope.should_recompose();
                #(#param_setup)*
                #recompose_setter
                if !__changed && __current_scope.has_composed_once() {
                    __composer.skip_current_group();
                    return;
                }
                #(#rebinds)*
                #helper_block
            }
        } else {
            quote! {
                #core_path::debug_label_current_scope(stringify!(#scope_label_ident));
                let __current_scope = __composer
                    .current_recompose_scope()
                    .expect("missing recompose scope");
                let mut __changed = __current_scope.should_recompose();
                #(#param_setup)*
                #recompose_setter
                let __result_slot_index = __composer
                    .__use_return_slot(|| #core_path::ReturnSlot::<#return_ty>::default());
                let __has_previous = __composer
                    .with_slot_value::<#core_path::ReturnSlot<#return_ty>, _>(
                        __result_slot_index,
                        |slot| slot.get().is_some(),
                    );
                if !__changed && __has_previous {
                    __composer.skip_current_group();
                    let __result = __composer
                        .with_slot_value::<#core_path::ReturnSlot<#return_ty>, _>(
                            __result_slot_index,
                            |slot| {
                                slot.get()
                                    .expect("composable return value missing during skip")
                            },
                        );
                    return __result;
                }
                let __value: #return_ty = {
                    #(#rebinds)*
                    #helper_block
                };
                __composer.with_slot_value_mut::<#core_path::ReturnSlot<#return_ty>, _>(
                    __result_slot_index,
                    |slot| {
                        slot.store(__value.clone());
                    },
                );
                __value
            }
        };

        let recompose_fn_body = if returns_unit {
            quote! {
                #(#param_setup_recompose)*
                #(#rebinds_for_recompose)*
                #recompose_block
                #recompose_setter
            }
        } else {
            quote! {
                #(#param_setup_recompose)*
                let __result_slot_index = __composer
                    .__use_return_slot(|| #core_path::ReturnSlot::<#return_ty>::default());
                #(#rebinds_for_recompose)*
                let __value: #return_ty = {
                    #recompose_block
                };
                __composer.with_slot_value_mut::<#core_path::ReturnSlot<#return_ty>, _>(
                    __result_slot_index,
                    |slot| {
                        slot.store(__value.clone());
                    },
                );
                #recompose_setter
                #invalidate_return_consumer
                __value
            }
        };

        let recompose_fn = quote! {
            #[allow(non_snake_case)]
            fn #recompose_fn_ident #impl_generics (
                __composer: &#core_path::Composer
            ) -> #return_ty #where_clause {
                #recompose_fn_body
            }
        };

        let helper_fn = quote! {
            #[allow(non_snake_case, clippy::too_many_arguments)]
            fn #helper_ident #impl_generics (
                __composer: &#core_path::Composer
                #(, #helper_inputs)*
            ) -> #return_ty #where_clause {
                #helper_body
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
            #core_path::with_current_composer(|__composer: &#core_path::Composer| {
                __composer.with_group(#key_expr, |__composer: &#core_path::Composer| {
                    #helper_ident(__composer #(, #wrapper_args)*)
                })
            })
        });
        *func.block = syn::parse2(wrapped).expect("failed to build block");
        TokenStream::from(quote! {
            #recompose_fn
            #helper_fn
            #func
        })
    } else {
        let wrapped = quote!({
            #caller_key_stmt
            #core_path::with_current_composer(|__outer_composer: &#core_path::Composer| {
                __outer_composer.with_group(#key_expr, |__composer: &#core_path::Composer| {
                    #core_path::debug_label_current_scope(stringify!(#scope_label_ident));
                    #(#rebinds_for_no_skip)*
                    #original_block
                })
            })
        });
        *func.block = syn::parse2(wrapped).expect("failed to build block");
        TokenStream::from(quote! { #func })
    }
}

fn find_reserved_pattern_ident(pat: &Pat) -> Option<&Ident> {
    use syn::visit::Visit;

    struct Scan<'ast> {
        found: Option<&'ast Ident>,
    }
    impl<'ast> syn::visit::Visit<'ast> for Scan<'ast> {
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

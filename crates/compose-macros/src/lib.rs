use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type};

/// Check if a type is Fn-like (impl FnMut/Fn/FnOnce, Box<dyn FnMut>, generic with Fn bound, etc.)
/// For generic type parameters (e.g., `F` where F: FnMut()), we need to check the bounds.
fn is_fn_like_type(ty: &Type) -> bool {
    match ty {
        // impl FnMut(...) + 'static, impl Fn(...), etc.
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
        // Box<dyn FnMut(...)>
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
        // bare fn(...) -> ...
        Type::BareFn(_) => true,
        _ => false,
    }
}

/// Check if a generic type parameter has Fn-like bounds by looking at the where clause and bounds
fn is_generic_fn_like(ty: &Type, generics: &syn::Generics) -> bool {
    // Extract the ident for Type::Path that might be a generic param
    let type_ident = match ty {
        Type::Path(type_path) if type_path.path.segments.len() == 1 => {
            &type_path.path.segments[0].ident
        }
        _ => return false,
    };

    // Check if it's a type parameter with Fn bounds
    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param {
            if type_param.ident == *type_ident {
                // Check the bounds on the type parameter
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

    // Also check where clause
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

#[proc_macro_attribute]
pub fn composable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_tokens = TokenStream2::from(attr);
    let mut enable_skip = true;
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
                *pat = Box::new(syn::parse_quote! { #ident });
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

    let original_block = func.block.clone();
    let helper_block = original_block.clone();
    let recompose_block = original_block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    // Rebinds will be generated later in the helper_body context where we have access to slots
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
    let _helper_ident = Ident::new(
        &format!("__compose_impl_{}", func.sig.ident),
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

    // Check if any params are impl Trait - if so, can't use skip optimization
    let has_impl_trait = param_info
        .iter()
        .any(|info| matches!(info.ty, Type::ImplTrait(_)));

    if enable_skip && !has_impl_trait {
        let helper_ident = Ident::new(
            &format!("__compose_impl_{}", func.sig.ident),
            Span::call_site(),
        );
        let generics = func.sig.generics.clone();
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let ty_generics_turbofish = ty_generics.as_turbofish();

        // Helper function signature: all params except impl Trait (which can't be named)
        let helper_inputs: Vec<TokenStream2> = param_info
            .iter()
            .filter_map(|info| {
                if info.is_impl_trait {
                    None
                } else {
                    let ident = &info.ident;
                    let ty = &info.ty;
                    Some(quote! { #ident: #ty })
                }
            })
            .collect();

        // Separate Fn-like params from regular params
        let param_state_slots: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_state_slot{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .map(|(info, slot_ident)| {
                if info.is_impl_trait {
                    quote! { __changed = true; }
                } else if is_fn_param(&info.ty, &generics) {
                    let ident = &info.ident;
                    quote! {
                        let #slot_ident = __composer
                            .use_value_slot(|| compose_core::CallbackHolder::new());
                        __composer.with_slot_value::<compose_core::CallbackHolder, _>(
                            #slot_ident,
                            |holder| {
                                holder.update(#ident);
                            },
                        );
                        __changed = true;
                    }
                } else {
                    let ident = &info.ident;
                    let ty = &info.ty;
                    quote! {
                        let #slot_ident = __composer
                            .use_value_slot(|| compose_core::ParamState::<#ty>::default());
                        if __composer.with_slot_value_mut::<compose_core::ParamState<#ty>, _>(
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
                if info.is_impl_trait {
                    quote! {}
                } else if is_fn_param(&info.ty, &generics) {
                    quote! {
                        let #slot_ident = __composer
                            .use_value_slot(|| compose_core::CallbackHolder::new());
                    }
                } else {
                    let ty = &info.ty;
                    quote! {
                        let #slot_ident = __composer
                            .use_value_slot(|| compose_core::ParamState::<#ty>::default());
                    }
                }
            })
            .collect();

        let rebinds: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_slots.iter())
            .map(|(info, slot_ident)| {
                if info.is_impl_trait {
                    quote! {}
                } else if is_fn_param(&info.ty, &generics) {
                    let pat = &info.pat;
                    let can_add_mut = matches!(pat.as_ref(), Pat::Ident(_));
                    if can_add_mut && !info.pat_is_mut {
                        quote! {
                            #[allow(unused_mut)]
                            let mut #pat = __composer
                                .with_slot_value::<compose_core::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    } else {
                        quote! {
                            #[allow(unused_mut)]
                            let #pat = __composer
                                .with_slot_value::<compose_core::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    }
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
                if info.is_impl_trait {
                    quote! {}
                } else if is_fn_param(&info.ty, &generics) {
                    let pat = &info.pat;
                    let can_add_mut = matches!(pat.as_ref(), Pat::Ident(_));
                    if can_add_mut && !info.pat_is_mut {
                        quote! {
                            #[allow(unused_mut)]
                            let mut #pat = __composer
                                .with_slot_value::<compose_core::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    } else {
                        quote! {
                            #[allow(unused_mut)]
                            let #pat = __composer
                                .with_slot_value::<compose_core::CallbackHolder, _>(
                                    #slot_ident,
                                    |holder| holder.clone_rc(),
                                );
                        }
                    }
                } else {
                    let pat = &info.pat;
                    let ty = &info.ty;
                    quote! {
                        let #pat = __composer
                            .with_slot_value::<compose_core::ParamState<#ty>, _>(
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
            &format!("__compose_recompose_{}", func.sig.ident),
            Span::call_site(),
        );

        let recompose_setter = quote! {
            {
                __composer.set_recompose_callback(move |
                    __composer: &compose_core::Composer|
                {
                    #recompose_fn_ident #ty_generics_turbofish (
                        __composer
                    );
                });
            }
        };

        let helper_body = quote! {
            let __current_scope = __composer
                .current_recompose_scope()
                .expect("missing recompose scope");
            let mut __changed = __current_scope.should_recompose();
            #(#param_setup)*
            let __result_slot_index = __composer
                .use_value_slot(|| compose_core::ReturnSlot::<#return_ty>::default());
            let __has_previous = __composer
                .with_slot_value::<compose_core::ReturnSlot<#return_ty>, _>(
                    __result_slot_index,
                    |slot| slot.get().is_some(),
                );
            if !__changed && __has_previous {
                __composer.skip_current_group();
                let __result = __composer
                    .with_slot_value::<compose_core::ReturnSlot<#return_ty>, _>(
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
            __composer.with_slot_value_mut::<compose_core::ReturnSlot<#return_ty>, _>(
                __result_slot_index,
                |slot| {
                    slot.store(__value.clone());
                },
            );
            #recompose_setter
            __value
        };

        let recompose_fn_body = quote! {
            #(#param_setup_recompose)*
            let __result_slot_index = __composer
                .use_value_slot(|| compose_core::ReturnSlot::<#return_ty>::default());
            #(#rebinds_for_recompose)*
            let __value: #return_ty = {
                #recompose_block
            };
            __composer.with_slot_value_mut::<compose_core::ReturnSlot<#return_ty>, _>(
                __result_slot_index,
                |slot| {
                    slot.store(__value.clone());
                },
            );
            #recompose_setter
            __value
        };

        let recompose_fn = quote! {
            #[allow(non_snake_case)]
            fn #recompose_fn_ident #impl_generics (
                __composer: &compose_core::Composer
            ) -> #return_ty #where_clause {
                #recompose_fn_body
            }
        };

        let helper_fn = quote! {
            #[allow(non_snake_case)]
            fn #helper_ident #impl_generics (
                __composer: &compose_core::Composer
                #(, #helper_inputs)*
            ) -> #return_ty #where_clause {
                #helper_body
            }
        };

        // Wrapper args: pass all params except impl Trait on initial call
        let wrapper_args: Vec<TokenStream2> = param_info
            .iter()
            .filter_map(|info| {
                if info.is_impl_trait {
                    None
                } else {
                    let ident = &info.ident;
                    Some(quote! { #ident })
                }
            })
            .collect();

        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &compose_core::Composer| {
                __composer.with_group(#key_expr, |__composer: &compose_core::Composer| {
                    #helper_ident(__composer #(, #wrapper_args)*)
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! {
            #recompose_fn
            #helper_fn
            #func
        })
    } else {
        // no_skip path: still uses simple rebinds
        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &compose_core::Composer| {
                __composer.with_group(#key_expr, |__scope: &compose_core::Composer| {
                    #(#rebinds_for_no_skip)*
                    #original_block
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! { #func })
    }
}

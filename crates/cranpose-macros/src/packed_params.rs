use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Ident, Type};

use crate::slot_key;

/// A composable call's parameters that are neither callbacks nor
/// `impl Trait`, kept together in one slot and compared field by field: one
/// slot lookup and one payload per call instead of one per parameter.
pub(crate) struct PackedParams<'a> {
    pub(crate) params: Vec<(&'a Ident, &'a Type)>,
    /// The call's return type when it returns a value, which the call keeps
    /// in its parameters' slot.
    pub(crate) returns: Option<&'a Type>,
}

impl PackedParams<'_> {
    pub(crate) fn slot() -> Ident {
        Ident::new("__params_slot", Span::mixed_site())
    }

    /// Whether the parameters' slot also holds what the call returned.
    pub(crate) fn holds_return(&self) -> bool {
        self.returns.is_some() && !self.params.is_empty()
    }

    pub(crate) fn state_type(&self, core_path: &TokenStream2) -> TokenStream2 {
        let types = self.params.iter().map(|(_, ty)| ty);
        match self.returns {
            Some(returns) => quote! { #core_path::ParamReturnState<(#(#types,)*), #returns> },
            None => quote! { #core_path::ParamState<(#(#types,)*)> },
        }
    }

    pub(crate) fn slot_stmt(&self, core_path: &TokenStream2, composer: &Ident) -> TokenStream2 {
        let state = self.state_type(core_path);
        let slot = Self::slot();
        let key = slot_key(core_path);
        quote! {
            let #slot = #composer.__use_param_slot(#key, || <#state>::default());
        }
    }

    /// `setup` led by the statements that store the parameters and mark the
    /// call changed when any differs from the last composition. When the
    /// slot holds the call's return, they also bind `previous` to what the
    /// call returned last if `skippable` and nothing changed, else `None`.
    pub(crate) fn with_setup(
        &self,
        core_path: &TokenStream2,
        composer: &Ident,
        (previous, skippable): (&Ident, bool),
        mut setup: Vec<TokenStream2>,
    ) -> Vec<TokenStream2> {
        if self.params.is_empty() {
            return setup;
        }
        let state = self.state_type(core_path);
        let slot = Self::slot();
        let key = slot_key(core_path);
        let param_state = Ident::new("__param_state", Span::mixed_site());
        let stored = Ident::new("__stored", Span::mixed_site());
        let idents = self.params.iter().map(|(ident, _)| ident);
        let refreshes = self.params.iter().enumerate().map(|(index, (ident, ty))| {
            param_field_refresh(core_path, &stored, (ident, ty), index, skippable)
        });
        let update = quote! {
            #param_state.update_fields(
                || (#(::core::clone::Clone::clone(&#idents),)*),
                |#stored| false #(| #refreshes)*,
            )
        };
        let statement = if !self.holds_return() {
            quote! {
                let (_, #slot) = #composer.__update_param_slot(
                    #key,
                    || <#state>::default(),
                    |#param_state: &mut #state| #update,
                );
                if #slot {
                    __changed = true;
                }
            }
        } else if skippable {
            quote! {
                let (#slot, #previous) = #composer.__update_param_slot(
                    #key,
                    || <#state>::default(),
                    |#param_state: &mut #state| {
                        if #update {
                            __changed = true;
                        }
                        (!__changed).then(|| #param_state.returned()).flatten()
                    },
                );
            }
        } else {
            quote! {
                let (#slot, ()) = #composer.__update_param_slot(
                    #key,
                    || <#state>::default(),
                    |#param_state: &mut #state| {
                        if #update {
                            __changed = true;
                        }
                    },
                );
            }
        };
        setup.insert(0, statement);
        setup
    }

    /// The statement that keeps `value` as what the call returned, in the
    /// parameters' slot.
    pub(crate) fn store_return(
        &self,
        core_path: &TokenStream2,
        composer: &Ident,
        value: &Ident,
    ) -> TokenStream2 {
        let state = self.state_type(core_path);
        let slot = Self::slot();
        quote! {
            #composer.with_slot_value_mut::<#state, _>(#slot, |state| {
                state.store_return(::core::clone::Clone::clone(&#value));
            });
        }
    }

    /// `setup` led by the statement that finds the parameters' slot.
    pub(crate) fn with_slot(
        &self,
        core_path: &TokenStream2,
        composer: &Ident,
        mut setup: Vec<TokenStream2>,
    ) -> Vec<TokenStream2> {
        if !self.params.is_empty() {
            setup.insert(0, self.slot_stmt(core_path, composer));
        }
        setup
    }

    /// The statements that read the stored parameters back for a
    /// recomposition started from the call's scope.
    pub(crate) fn reads(&self, core_path: &TokenStream2, composer: &Ident) -> Vec<TokenStream2> {
        if self.params.is_empty() {
            return Vec::new();
        }
        let state = self.state_type(core_path);
        let slot = Self::slot();
        let param_state = Ident::new("__param_state", Span::mixed_site());
        let idents = self.params.iter().map(|(ident, _)| ident);
        vec![quote! {
            let (#(#idents,)*) = #composer.with_slot_value::<#state, _>(#slot, |#param_state| {
                #param_state
                    .value()
                    .expect("composable parameter missing for recomposition")
            });
        }]
    }
}

/// The call that brings field `index` of a call's stored parameters up to
/// date with `ident` and says whether it changed: `refresh_shared_param` for
/// an `Rc` or `Arc`, whose unchanged allocation needs no comparison of its
/// contents.
/// The statement that refreshes one stored parameter. A call that cannot
/// skip only stores it: no comparison would change what the call does.
fn param_field_refresh(
    core_path: &TokenStream2,
    stored: &Ident,
    (ident, ty): (&Ident, &Type),
    index: usize,
    skippable: bool,
) -> TokenStream2 {
    let index = syn::Index::from(index);
    if !skippable {
        quote! { #core_path::store_param(&mut #stored.#index, &#ident) }
    } else if is_shared_pointer(ty) {
        quote! { #core_path::refresh_shared_param(&mut #stored.#index, &#ident) }
    } else {
        quote! { #core_path::refresh_param(&mut #stored.#index, &#ident) }
    }
}

/// Whether a parameter type is an `Rc<..>` or `Arc<..>`.
fn is_shared_pointer(ty: &Type) -> bool {
    match ty {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Rc" || segment.ident == "Arc"),
        _ => false,
    }
}

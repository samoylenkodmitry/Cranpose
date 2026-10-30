use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Ident, Type};

use crate::PackedParams;

/// The code of a call that returns a value: it keeps what its body
/// returned in its parameters' slot when it has packed parameters, else in
/// a return slot of its own, and a call that skips its body returns it.
pub(crate) struct ReturningCall<'a> {
    pub(crate) core_path: &'a TokenStream2,
    pub(crate) composer: &'a Ident,
    pub(crate) group: &'a Ident,
    pub(crate) current_scope: &'a Ident,
    pub(crate) scope_label: &'a Ident,
    pub(crate) return_ty: &'a Type,
    pub(crate) slot_key: &'a TokenStream2,
    pub(crate) packed: &'a PackedParams<'a>,
    pub(crate) skippable: bool,
    pub(crate) body_call: &'a TokenStream2,
}

impl ReturningCall<'_> {
    /// What the call returned last, when it may skip its body.
    pub(crate) fn previous() -> Ident {
        Ident::new("__previous", Span::mixed_site())
    }

    pub(crate) fn value() -> Ident {
        Ident::new("__value", Span::mixed_site())
    }

    pub(crate) fn return_slot() -> Ident {
        Ident::new("__result_slot_index", Span::mixed_site())
    }

    pub(crate) fn slot_type(&self) -> TokenStream2 {
        let (core_path, return_ty) = (self.core_path, self.return_ty);
        quote! { #core_path::ReturnSlot<#return_ty> }
    }

    /// Returns what the call returned last, when it may skip its body.
    pub(crate) fn skip(&self) -> Option<TokenStream2> {
        let (group, previous) = (self.group, Self::previous());
        let result = Ident::new("__result", Span::mixed_site());
        (self.skippable || !self.packed.holds_return()).then(|| {
            quote! {
                if let ::core::option::Option::Some(#result) = #previous {
                    #group.skip();
                    return #result;
                }
            }
        })
    }

    /// Keeps the value the body returned.
    pub(crate) fn store(&self) -> TokenStream2 {
        let (composer, value) = (self.composer, Self::value());
        if self.packed.holds_return() {
            return self.packed.store_return(self.core_path, composer, &value);
        }
        let (slot, slot_type) = (Self::return_slot(), self.slot_type());
        quote! {
            #composer.with_slot_value_mut::<#slot_type, _>(#slot, |slot| {
                slot.store(::core::clone::Clone::clone(&#value));
            });
        }
    }

    pub(crate) fn helper_body(
        &self,
        param_setup: &[TokenStream2],
        recompose_setter: &TokenStream2,
    ) -> TokenStream2 {
        let (core_path, composer, scope_label) = (self.core_path, self.composer, self.scope_label);
        let (current_scope, return_ty, body_call) =
            (self.current_scope, self.return_ty, self.body_call);
        let (previous, value, slot) = (Self::previous(), Self::value(), Self::return_slot());
        let lookup = (!self.packed.holds_return()).then(|| {
            let (slot_key, slot_type) = (self.slot_key, self.slot_type());
            quote! {
                let (#slot, #previous) = #composer.__update_return_slot(
                    #slot_key,
                    || <#slot_type>::default(),
                    |slot: &mut #slot_type| (!__changed).then(|| slot.get()).flatten(),
                );
            }
        });
        let (skip, store) = (self.skip(), self.store());
        quote! {
            #core_path::debug_label_current_scope(stringify!(#scope_label));
            let mut __changed = #current_scope.should_recompose();
            #(#param_setup)*
            #recompose_setter
            #lookup
            #skip
            let #value: #return_ty = #body_call;
            #store
            #value
        }
    }

    pub(crate) fn recompose_body(
        &self,
        param_setup: &[TokenStream2],
        reads: &[TokenStream2],
        invalidate_return_consumer: &TokenStream2,
    ) -> TokenStream2 {
        let (composer, return_ty, body_call) = (self.composer, self.return_ty, self.body_call);
        let lookup = (!self.packed.holds_return()).then(|| {
            let (slot, slot_type) = (Self::return_slot(), self.slot_type());
            quote! {
                let #slot = #composer.__use_return_slot(|| <#slot_type>::default());
            }
        });
        let (value, store) = (Self::value(), self.store());
        quote! {
            #(#param_setup)*
            #lookup
            #(#reads)*
            let #value: #return_ty = #body_call;
            #store
            #invalidate_return_consumer
            #value
        }
    }
}

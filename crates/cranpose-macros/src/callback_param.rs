use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Ident, Pat};

use crate::{
    ParamInfo, held_callback::HeldCallback, is_fn_param, is_generic_fn_like, slot_key,
    type_bare_generic_ident,
};

// How a call hands a callback parameter to its holder.
enum CallbackPass<'a> {
    // Boxed as the `FnMut` the held callback is bounded to be.
    Held(&'a HeldCallback),
    // A generic `FnMut()`, boxed.
    Boxed,
    // A callback type the holder takes as it is.
    Direct,
}

// A callback parameter of a skippable composable. The call keeps it in a
// `CallbackHolder` slot, and the body reads it through that slot.
pub(crate) struct CallbackParam<'a> {
    info: &'a ParamInfo,
    pub(crate) slot: &'a Ident,
    pass: CallbackPass<'a>,
    holder_type: TokenStream2,
}

impl<'a> CallbackParam<'a> {
    // The callback `info` is, or `None` for a parameter the call compares.
    pub(crate) fn of(
        core_path: &TokenStream2,
        info: &'a ParamInfo,
        slot: &'a Ident,
        held: Option<&'a HeldCallback>,
        generics: &syn::Generics,
    ) -> Option<Self> {
        let pass = match held {
            Some(held) => CallbackPass::Held(held),
            None if info.is_impl_trait => return None,
            None if type_bare_generic_ident(&info.ty).is_some()
                && is_generic_fn_like(&info.ty, generics) =>
            {
                CallbackPass::Boxed
            }
            None if is_fn_param(&info.ty, generics) => CallbackPass::Direct,
            None => return None,
        };
        let holder_type = match &pass {
            CallbackPass::Held(held) => {
                let dyn_type = held.dyn_type();
                quote! { #core_path::CallbackHolder<#dyn_type> }
            }
            CallbackPass::Boxed | CallbackPass::Direct => quote! { #core_path::CallbackHolder },
        };
        Some(Self {
            info,
            slot,
            pass,
            holder_type,
        })
    }

    // Whether the call boxes the callback before it reaches the holder.
    pub(crate) fn boxes(&self) -> bool {
        !matches!(self.pass, CallbackPass::Direct)
    }

    // The trait object the call boxes the callback as, or `None` when the
    // call passes it as it is.
    pub(crate) fn boxed_type(&self) -> Option<TokenStream2> {
        match &self.pass {
            CallbackPass::Held(held) => Some(held.dyn_type()),
            CallbackPass::Boxed => Some(quote! { dyn ::core::ops::FnMut() + 'static }),
            CallbackPass::Direct => None,
        }
    }

    // Stores the callback in its slot. A callback that never ran outside an
    // event handler leaves the call unchanged: its forwarder calls the new one.
    pub(crate) fn setup(&self, core_path: &TokenStream2, composer: &Ident) -> TokenStream2 {
        let Self {
            slot, holder_type, ..
        } = self;
        let ident = &self.info.ident;
        let slot_key = slot_key(core_path);
        let hold = match self.pass {
            CallbackPass::Held(_) | CallbackPass::Boxed => {
                quote! { <#holder_type>::__hold(#composer, #slot_key, #ident) }
            }
            CallbackPass::Direct => quote! {
                #composer.__update_param_slot(
                    #slot_key,
                    || <#holder_type>::new(),
                    |holder: &mut #holder_type| {
                        holder.update(#ident);
                        holder.renders()
                    },
                )
            },
        };
        quote! {
            let (#slot, __renders) = #hold;
            if __renders {
                __changed = true;
            }
        }
    }

    // Finds the slot again for a recomposition started from the call's scope.
    pub(crate) fn recompose_setup(
        &self,
        core_path: &TokenStream2,
        composer: &Ident,
    ) -> TokenStream2 {
        let Self {
            slot, holder_type, ..
        } = self;
        let slot_key = slot_key(core_path);
        quote! {
            let #slot = <#holder_type>::__slot(#composer, #slot_key);
        }
    }

    // The body's parameter that takes the slot.
    pub(crate) fn body_input(&self, core_path: &TokenStream2) -> TokenStream2 {
        let Self {
            slot, holder_type, ..
        } = self;
        quote! { #slot: #core_path::ValueSlotHandle<'_, #holder_type> }
    }

    // Binds the parameter's own pattern in the body to the stored callback.
    pub(crate) fn rebind(&self, core_path: &TokenStream2, composer: &Ident) -> TokenStream2 {
        let Self {
            slot, holder_type, ..
        } = self;
        let pat = &self.info.pat;
        let binding = if matches!(pat.as_ref(), Pat::Ident(_)) && !self.info.pat_is_mut {
            quote! { mut #pat }
        } else {
            quote! { #pat }
        };
        let value = match self.pass {
            CallbackPass::Held(held) => held.forwarder(&quote! {
                <#holder_type>::__forwarder(#composer, #slot)
            }),
            CallbackPass::Boxed | CallbackPass::Direct => quote! {
                #composer.with_slot_value::<#holder_type, _>(
                    #slot,
                    #core_path::CallbackHolder::clone_rc,
                )
            },
        };
        quote! {
            #[allow(unused_mut, reason = "only some callbacks change the binding")]
            let #binding = #value;
        }
    }
}

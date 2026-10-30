use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{Ident, Type};

use crate::type_bare_generic_ident;

// Whether a type holds a borrow that may end before `'static`.
pub(crate) fn borrows(ty: &Type) -> bool {
    use syn::visit::Visit;

    struct Scan(bool);
    impl<'ast> Visit<'ast> for Scan {
        fn visit_type_reference(&mut self, reference: &'ast syn::TypeReference) {
            if reference
                .lifetime
                .as_ref()
                .is_none_or(|lifetime| lifetime.ident != "static")
            {
                self.0 = true;
            }
            syn::visit::visit_type_reference(self, reference);
        }

        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            if lifetime.ident != "static" {
                self.0 = true;
            }
        }

        // A callback's arguments are borrowed only while it runs.
        fn visit_parenthesized_generic_arguments(
            &mut self,
            _: &'ast syn::ParenthesizedGenericArguments,
        ) {
        }
    }
    let mut scan = Scan(false);
    scan.visit_type(ty);
    scan.0
}

// A callback parameter a call keeps in a `CallbackHolder`: `impl Fn(..)` or
// `impl FnMut(..)` that takes its arguments by value and returns nothing,
// bounded by `'static` and at most `Clone` beside. Its body calls it through
// a forwarder to the callback stored last, so a call whose callbacks only ran
// in event handlers skips its body when only they changed. A callback that
// returns a value or borrows an argument is read while its composable
// composes, measures or draws, so it would never let the call skip; such a
// call keeps the inline path.
pub(crate) struct HeldCallback {
    inputs: Vec<Type>,
}

impl HeldCallback {
    // The callback an `impl Trait` parameter, or one whose type is a generic
    // parameter of the function, is bounded to be.
    pub(crate) fn of(ty: &Type, generics: &syn::Generics) -> Option<Self> {
        match ty {
            Type::ImplTrait(impl_trait) => Self::of_bounds(impl_trait.bounds.iter()),
            _ => {
                let ident = type_bare_generic_ident(ty)?;
                let declared = generics.params.iter().filter_map(|param| match param {
                    syn::GenericParam::Type(param) if param.ident == *ident => {
                        Some(param.bounds.iter())
                    }
                    _ => None,
                });
                let predicates = generics
                    .where_clause
                    .iter()
                    .flat_map(|clause| clause.predicates.iter())
                    .filter_map(|predicate| match predicate {
                        syn::WherePredicate::Type(predicate)
                            if type_bare_generic_ident(&predicate.bounded_ty) == Some(ident) =>
                        {
                            Some(predicate.bounds.iter())
                        }
                        _ => None,
                    });
                Self::of_bounds(declared.chain(predicates).flatten())
            }
        }
    }

    pub(crate) fn of_bounds<'a>(
        bounds: impl Iterator<Item = &'a syn::TypeParamBound>,
    ) -> Option<Self> {
        let mut callback = None;
        let mut is_static = false;
        for bound in bounds {
            match bound {
                syn::TypeParamBound::Lifetime(lifetime) if lifetime.ident == "static" => {
                    is_static = true;
                }
                syn::TypeParamBound::Trait(trait_bound) => {
                    let segment = trait_bound.path.segments.last()?;
                    match &segment.arguments {
                        syn::PathArguments::None if segment.ident == "Clone" => {}
                        syn::PathArguments::Parenthesized(args)
                            if callback.is_none()
                                && trait_bound.lifetimes.is_none()
                                && (segment.ident == "Fn" || segment.ident == "FnMut") =>
                        {
                            callback = Some(Self::of_args(args)?);
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        callback.filter(|_| is_static)
    }

    fn of_args(args: &syn::ParenthesizedGenericArguments) -> Option<Self> {
        let by_value = args
            .inputs
            .iter()
            .all(|arg| !matches!(arg.ty, Type::Reference(_)));
        (by_value && crate::returns_unit(&args.output)).then(|| Self {
            inputs: args.inputs.iter().map(|arg| arg.ty.clone()).collect(),
        })
    }

    // The trait object the holder keeps the callback as.
    pub(crate) fn dyn_type(&self) -> TokenStream2 {
        let inputs = &self.inputs;
        quote! { dyn ::core::ops::FnMut(#(#inputs),*) + 'static }
    }

    // A closure that calls the callback stored last through `forwarder`.
    pub(crate) fn forwarder(&self, forwarder: &TokenStream2) -> TokenStream2 {
        let args: Vec<Ident> = (0..self.inputs.len())
            .map(|index| Ident::new(&format!("__arg{index}"), Span::mixed_site()))
            .collect();
        let inputs = &self.inputs;
        quote! {{
            let __forwarder = #forwarder;
            move |#(#args: #inputs),*| __forwarder.call(|__callback| __callback(#(#args),*))
        }}
    }
}

//! `#[worker_job]`: a function a `LaunchedEffect` scope runs off the UI
//! thread on every platform.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, ReturnType};

pub(crate) fn expand(item: &ItemFn) -> syn::Result<TokenStream> {
    let core = crate::core_crate_path();
    let signature = &item.sig;
    if let Some(generics) = signature.generics.params.first() {
        return Err(syn::Error::new_spanned(
            generics,
            "a worker job takes no generic parameters: a Web Worker finds it by its name",
        ));
    }
    if let Some(token) = &signature.asyncness {
        return Err(syn::Error::new_spanned(
            token,
            "a worker job is a plain function: it computes and returns",
        ));
    }
    let mut inputs = signature.inputs.iter();
    let (Some(FnArg::Typed(input)), None) = (inputs.next(), inputs.next()) else {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            "a worker job takes one parameter, its input",
        ));
    };
    let output = match &signature.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, output) => quote!(#output),
    };
    let name = &signature.ident;
    let job_name = name.to_string();
    let exported = format!("cranpose_worker_job_{job_name}");
    let export = format_ident!("__cranpose_worker_job_{}", name);
    let (attributes, visibility, body) = (&item.attrs, &item.vis, &item.block);
    let (pattern, input_type) = (&input.pat, &input.ty);
    Ok(quote! {
        #(#attributes)*
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy)]
        #visibility struct #name;

        impl #name {
            #[doc = "The job with its input: the work `withBlocking` runs."]
            #visibility fn with(self, input: #input_type) -> #core::WorkerCall<Self> {
                #core::__worker::call(input)
            }
        }

        impl #core::WorkerJob for #name {
            type Input = #input_type;
            type Output = #output;
            const NAME: &'static str = #job_name;

            fn run(#pattern: #input_type) -> #output #body
        }

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        const _: () = {
            use #core::__worker::wasm_bindgen;

            #[wasm_bindgen::prelude::wasm_bindgen(js_name = #exported)]
            pub fn #export(input: Vec<u8>) -> Vec<u8> {
                #core::__worker::run_bytes::<#name>(input)
            }
        };
    })
}

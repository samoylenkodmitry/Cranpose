use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{FnArg, ItemFn, ItemImpl, Pat, ReturnType, Type};

fn runtime_path() -> TokenStream {
    match proc_macro_crate::crate_name("cranpose-live") {
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote!(::#ident)
        }
        _ => quote!(::cranpose_live),
    }
}

fn unit(output: &ReturnType) -> bool {
    matches!(output, ReturnType::Default)
        || matches!(output, ReturnType::Type(_, ty) if matches!(&**ty, Type::Tuple(tuple) if tuple.elems.is_empty()))
}

fn parameter(argument: &syn::PatType) -> syn::Result<&syn::Ident> {
    match &*argument.pat {
        Pat::Ident(pattern) if pattern.subpat.is_none() => Ok(&pattern.ident),
        other => Err(syn::Error::new_spanned(
            other,
            "live arguments need plain names",
        )),
    }
}

pub(crate) fn component(function: &ItemFn) -> TokenStream {
    if proc_macro_crate::crate_name("cranpose-live").is_err() {
        return TokenStream::new();
    }
    let runtime = runtime_path();
    let name = &function.sig.ident;
    let signature = function.sig.to_token_stream().to_string();
    let cfg = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"));
    let documentation = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(value) => match &value.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (parameters, content, unavailable, renderer, render) = match callable(function) {
        Ok(adapter) => {
            let Adapter {
                parameters,
                content,
                setup,
                arguments,
            } = adapter;
            let content = match content {
                Some(name) => quote!(Some(#name.into())),
                None => quote!(None),
            };
            let renderer = quote! {
                fn render(_call: #runtime::Invocation) -> Result<(), #runtime::Error> {
                    #(#setup)*
                    #name(#(#arguments),*);
                    Ok(())
                }
            };
            (
                parameters,
                content,
                quote!(None),
                renderer,
                quote!(Some(render)),
            )
        }
        Err(error) => {
            let reason = error.to_string();
            (
                Vec::new(),
                quote!(None),
                quote!(Some(#reason.into())),
                quote!(),
                quote!(None),
            )
        }
    };
    quote! {
        #(#cfg)*
        const _: () = {
            fn schema() -> #runtime::ComponentSchema {
                #runtime::ComponentSchema {
                    name: concat!(module_path!(), "::", stringify!(#name)).into(),
                    documentation: #documentation.into(),
                    signature: #signature.into(),
                    unavailable: #unavailable,
                    parameters: vec![#(#parameters),*],
                    content: #content,
                }
            }
            #renderer
            #runtime::__submit! { #runtime::Component { schema, render: #render } }
        };
    }
}

struct Adapter {
    parameters: Vec<TokenStream>,
    arguments: Vec<TokenStream>,
    setup: Vec<TokenStream>,
    content: Option<String>,
}

fn callable(function: &ItemFn) -> syn::Result<Adapter> {
    if !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
        || matches!(function.sig.safety, syn::Safety::Unsafe(_))
        || !unit(&function.sig.output)
    {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "live components need a safe, synchronous, non-generic, unit-returning signature",
        ));
    }
    let runtime = runtime_path();
    let mut parameters = Vec::new();
    let mut arguments = Vec::new();
    let mut setup = Vec::new();
    let mut content = None;
    for (index, argument) in function.sig.inputs.iter().enumerate() {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "live components must be free functions",
            ));
        };
        let ident = parameter(argument)?;
        let field = ident.to_string();
        let ty = &argument.ty;
        if matches!(&**ty, Type::ImplTrait(_)) {
            if !crate::is_zero_arg_fn_impl_trait(ty) {
                return Err(syn::Error::new_spanned(
                    ty,
                    "live callbacks currently take no arguments",
                ));
            }
            if field == "content" {
                if index + 1 != function.sig.inputs.len() {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "live content currently needs to be the last parameter",
                    ));
                }
                content = Some(field);
                setup.push(quote!(let #ident = _call.content();));
                arguments.push(quote!(move || #ident.render()));
            } else {
                parameters.push(quote!(#runtime::Parameter::action(#field)));
                setup.push(quote!(let #ident = _call.action(#field)?;));
                arguments.push(quote!(move || #ident.invoke()));
            }
        } else {
            if !matches!(&**ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| matches!(segment.ident.to_string().as_str(), "String" | "i64" | "f64" | "bool")))
            {
                return Err(syn::Error::new_spanned(
                    ty,
                    "this native argument type needs a live adapter",
                ));
            }
            parameters.push(quote!(#runtime::Parameter::value::<#ty>(#field)));
            arguments.push(quote!(_call.argument::<#ty>(#field)?));
        }
    }
    Ok(Adapter {
        parameters,
        arguments,
        setup,
        content,
    })
}

pub(crate) fn api(mut implementation: ItemImpl) -> syn::Result<TokenStream> {
    if implementation.trait_.is_some() || !implementation.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &implementation.self_ty,
            "live_api needs a concrete inherent impl",
        ));
    }
    let runtime = runtime_path();
    let ty = &implementation.self_ty;
    let mut exports = Vec::new();
    for item in &mut implementation.items {
        if let syn::ImplItem::Fn(method) = item
            && let Some(export) = export_method(method)?
        {
            exports.push(export);
        }
    }
    if exports.is_empty() {
        return Err(syn::Error::new_spanned(
            &implementation,
            "live_api needs at least one live member",
        ));
    }
    let cfg = configuration_attributes(&implementation.attrs);
    Ok(quote! {
        #implementation
        #(#cfg)*
        impl #runtime::Api for #ty {
            fn register(self: ::std::rc::Rc<Self>, builder: &mut #runtime::ApiBuilder) -> Result<(), #runtime::Error> {
                #(#exports)*
                Ok(())
            }
        }
    })
}

fn configuration_attributes(
    attributes: &[syn::Attribute],
) -> impl Iterator<Item = &syn::Attribute> {
    attributes
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}

fn export_method(method: &mut syn::ImplItemFn) -> syn::Result<Option<TokenStream>> {
    let mut kinds = method
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("live"));
    let Some(attribute) = kinds.next() else {
        return Ok(None);
    };
    if kinds.next().is_some() {
        return Err(syn::Error::new_spanned(
            method,
            "one live annotation is allowed per method",
        ));
    }
    let kind = attribute.parse_args::<syn::Ident>()?;
    method.attrs.retain(|attr| !attr.path().is_ident("live"));
    let name = &method.sig.ident;
    let field = name.to_string();
    if !method
        .sig
        .receiver()
        .is_some_and(|r| matches!(r.kind, syn::ReceiverKind::Reference(_, _, None)))
        || !method.sig.generics.params.is_empty()
        || method.sig.asyncness.is_some()
    {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "live API methods currently need synchronous &self signatures",
        ));
    }
    let export = match kind.to_string().as_str() {
        "state" if method.sig.inputs.len() == 1 => quote!(builder.state(#field, self.#name())?;),
        "action" if unit(&method.sig.output) => action_adapter(method)?,
        _ => {
            return Err(syn::Error::new_spanned(
                kind,
                "use live(state) for a parameterless StateFlow getter, or live(action) for a unit-returning action",
            ));
        }
    };
    let cfg = configuration_attributes(&method.attrs);
    Ok(Some(quote! { #(#cfg)* { #export } }))
}

fn action_adapter(method: &syn::ImplItemFn) -> syn::Result<TokenStream> {
    let runtime = runtime_path();
    let name = &method.sig.ident;
    let field = name.to_string();
    let mut parameters = Vec::new();
    let mut arguments = Vec::new();
    for (index, input) in method.sig.inputs.iter().skip(1).enumerate() {
        let FnArg::Typed(input) = input else { continue };
        let name = parameter(input)?.to_string();
        let ty = &input.ty;
        parameters.push(quote!(#runtime::Parameter::value::<#ty>(#name)));
        arguments.push(quote!(#runtime::__argument::<#ty>(_arguments, #index)?));
    }
    Ok(quote! {
        let model = ::std::rc::Rc::clone(&self);
        builder.action(#field, vec![#(#parameters),*], move |_arguments| {
            model.#name(#(#arguments),*);
            Ok(())
        })?;
    })
}

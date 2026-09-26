use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, ItemFn, LitBool, LitInt, LitStr, ReturnType};

struct Options {
    name: LitStr,
    group: LitStr,
    width: u32,
    height: u32,
    dark: bool,
}

impl Options {
    fn parse(args: TokenStream, ident: &Ident) -> syn::Result<Self> {
        let mut options = Self {
            name: LitStr::new(&ident.to_string(), ident.span()),
            group: LitStr::new("", ident.span()),
            width: 480,
            height: 640,
            dark: false,
        };
        let parser = syn::meta::parser(|meta| options.parse_option(meta));
        syn::parse::Parser::parse2(parser, args)?;
        options.validate_dimensions(ident)?;
        Ok(options)
    }

    fn parse_option(&mut self, meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
        if meta.path.is_ident("name") {
            self.name = meta.value()?.parse::<LitStr>()?;
        } else if meta.path.is_ident("group") {
            self.group = meta.value()?.parse::<LitStr>()?;
        } else if meta.path.is_ident("width") {
            self.width = meta.value()?.parse::<LitInt>()?.base10_parse()?;
        } else if meta.path.is_ident("height") {
            self.height = meta.value()?.parse::<LitInt>()?.base10_parse()?;
        } else if meta.path.is_ident("dark") {
            self.dark = meta.value()?.parse::<LitBool>()?.value;
        } else {
            return Err(meta.error("supported preview options: name, group, width, height, dark"));
        }
        Ok(())
    }

    fn validate_dimensions(&self, ident: &Ident) -> syn::Result<()> {
        if !(1..=8192).contains(&self.width) || !(1..=8192).contains(&self.height) {
            return Err(syn::Error::new_spanned(
                ident,
                "preview dimensions must be between 1 and 8192",
            ));
        }
        Ok(())
    }
}

fn validate(function: &ItemFn) -> syn::Result<()> {
    if !function.sig.inputs.is_empty()
        || !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
    {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "preview functions must be synchronous, have no parameters, and have no generics",
        ));
    }
    if !matches!(&function.sig.output, ReturnType::Default)
        && !matches!(&function.sig.output, ReturnType::Type(_, ty) if matches!(&**ty, syn::Type::Tuple(tuple) if tuple.elems.is_empty()))
    {
        return Err(syn::Error::new_spanned(
            &function.sig.output,
            "preview functions must return ()",
        ));
    }
    Ok(())
}

pub(crate) fn expand(args: TokenStream, function: ItemFn) -> syn::Result<TokenStream> {
    validate(&function)?;
    let ident = &function.sig.ident;
    let Options {
        name,
        group,
        width,
        height,
        dark,
    } = Options::parse(args, ident)?;
    let path = crate::core_crate_path();
    Ok(quote! {
        #function
        #path::preview::__submit! {
            #path::preview::Preview {
                id: concat!(module_path!(), "::", stringify!(#ident), "@", #name, "/", stringify!(#width), "x", stringify!(#height), "/", stringify!(#dark)),
                name: #name, group: #group, function: stringify!(#ident),
                file: file!(), line: line!(), width: #width, height: #height, dark: #dark,
                render: #ident,
            }
        }
    })
}

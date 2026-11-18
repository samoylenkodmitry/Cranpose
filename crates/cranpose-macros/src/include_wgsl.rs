// `include_wgsl!`: WGSL files embedded as one string without their comments.

use std::{fs, path::Path};

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{LitStr, Token, parse::Parser, punctuated::Punctuated};

// The files named in `input`, found beside the file at `caller`, stripped
// and joined in order. The block also includes each file's bytes in a
// constant nothing reads, so cargo builds again when a file changes.
pub(crate) fn include_wgsl(input: TokenStream, caller: Option<&Path>) -> syn::Result<TokenStream> {
    let names = Punctuated::<LitStr, Token![,]>::parse_terminated.parse2(input)?;
    if names.is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "expected the paths of one or more WGSL files",
        ));
    }
    let directory = caller.and_then(Path::parent).ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
            "the calling file is not on the local file system",
        )
    })?;
    let mut text = String::new();
    let mut tracked = Vec::with_capacity(names.len());
    for name in &names {
        let error = |message: String| syn::Error::new(name.span(), message);
        let file = fs::canonicalize(directory.join(name.value()))
            .map_err(|failure| error(format!("{}: {failure}", name.value())))?;
        let source = fs::read_to_string(&file)
            .map_err(|failure| error(format!("{}: {failure}", file.display())))?;
        strip_into(&source, &mut text)
            .map_err(|message| error(format!("{}: {message}", name.value())))?;
        tracked.push(file.to_string_lossy().into_owned());
    }
    Ok(quote!({
        #(const _: &[u8] = include_bytes!(#tracked);)*
        #text
    }))
}

// Appends `source` without its comments or the whitespace round each line,
// one line for each of its lines, so a compiler's line numbers still point
// into a file included alone. WGSL has no string literals, so `//` always
// opens a comment.
fn strip_into(source: &str, text: &mut String) -> Result<(), &'static str> {
    if source.contains("/*") {
        return Err("block comments are not stripped; use `//`");
    }
    for line in source.lines() {
        let code = line.split_once("//").map_or(line, |(code, _)| code);
        text.push_str(code.trim());
        text.push('\n');
    }
    Ok(())
}

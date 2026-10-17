//! Modifier chains of constants, built once.
//!
//! A chain such as `Modifier::empty().fill_max_width().height(16.0)` gives
//! the same modifier every time it runs when each builder reads only its
//! receiver and arguments and each argument is a constant. Rebuilt on each
//! recomposition, it costs an allocation an element, and the node it goes to
//! compares it element by element with the one it holds and drops it. The
//! body keeps such a chain in a slot instead: the first composition builds
//! it, later ones hand out the same chain, which the node finds equal by
//! identity.
//!
//! Only the builders in [`PURE_BUILDERS`] take part. They are inherent
//! methods of `Modifier`, which no trait method can stand in for, and a test
//! in cranpose-ui checks that each one builds the same modifier under other
//! densities and layout directions.

use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::{
    Expr, ExprCall, ExprMethodCall, Lit, UnOp,
    visit_mut::{self, VisitMut},
};

/// The `Modifier` builders that read nothing but their receiver and their
/// arguments.
pub(crate) const PURE_BUILDERS: &[&str] = &[
    "align",
    "background",
    "border",
    "clip_to_bounds",
    "fill_max_height",
    "fill_max_height_fraction",
    "fill_max_size",
    "fill_max_size_fraction",
    "fill_max_width",
    "fill_max_width_fraction",
    "height",
    "height_in",
    "offset",
    "padding",
    "padding_each",
    "padding_horizontal",
    "padding_symmetric",
    "padding_vertical",
    "required_height",
    "required_size",
    "required_width",
    "rounded_corner_shape",
    "rounded_corners",
    "size",
    "size_in",
    "size_points",
    "weight",
    "weight_with_fill",
    "width",
    "width_in",
    "wrap_content_height",
    "wrap_content_size",
    "wrap_content_width",
];

/// The functions that make a constant from constants, besides tuple struct
/// and enum variant constructors.
const CONSTANT_CALLS: &[&[&str]] = &[
    &["Color", "rgb"],
    &["Color", "rgba"],
    &["Color", "from_rgb_u8"],
    &["Color", "from_rgba_u8"],
];

/// Keeps each modifier chain of constants in `block`, outside closures and
/// nested items, in a slot of the composition.
pub(crate) fn keep_constant_modifiers(core_path: &TokenStream2, block: &mut syn::Block) {
    let mut keeper = Keeper {
        core_path,
        next_index: 0,
    };
    keeper.visit_block_mut(block);
}

struct Keeper<'a> {
    core_path: &'a TokenStream2,
    next_index: u32,
}

impl VisitMut for Keeper<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            // These run later, elsewhere or inside a slot's initializer,
            // where the body's composer is not theirs to use.
            Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) | Expr::Macro(_) => {}
            _ if is_constant_chain(expr) => {
                let index = self.next_index;
                self.next_index += 1;
                let core_path = self.core_path;
                let composer = syn::Ident::new("__composer", Span::mixed_site());
                let key = syn::Ident::new("__CRANPOSE_CONSTANT_KEY", Span::mixed_site());
                let chain = std::mem::replace(expr, Expr::Verbatim(TokenStream2::new()));
                *expr = syn::parse_quote! {
                    #composer.__constant(
                        {
                            const #key: #core_path::Key =
                                #core_path::constant_value_key(file!(), line!(), column!(), #index);
                            #key
                        },
                        || #chain,
                    )
                };
            }
            _ => visit_mut::visit_expr_mut(self, expr),
        }
    }

    fn visit_item_mut(&mut self, _item: &mut syn::Item) {}
}

/// Whether `expr` is `Modifier::empty()` followed by builders of
/// [`PURE_BUILDERS`] whose arguments are all constants.
fn is_constant_chain(expr: &Expr) -> bool {
    let mut current = expr;
    let mut builders = 0;
    loop {
        match current {
            Expr::MethodCall(ExprMethodCall {
                receiver,
                method,
                turbofish: None,
                args,
                ..
            }) if PURE_BUILDERS.contains(&method.to_string().as_str())
                && args.iter().all(is_constant) =>
            {
                builders += 1;
                current = receiver;
            }
            Expr::Call(ExprCall { func, args, .. }) if args.is_empty() => {
                return builders > 0 && path_is(func, &["Modifier", "empty"]);
            }
            _ => return false,
        }
    }
}

/// Whether `expr` gives the same value each time it runs: a literal, a
/// negated one, a constant or a unit variant by its path, or a constructor
/// or a [`CONSTANT_CALLS`] function given constants.
fn is_constant(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) => true,
        Expr::Unary(unary) => {
            matches!(unary.op, UnOp::Neg(_))
                && matches!(&*unary.expr, Expr::Lit(lit) if is_number(&lit.lit))
        }
        Expr::Paren(inner) => is_constant(&inner.expr),
        Expr::Group(inner) => is_constant(&inner.expr),
        Expr::Path(path) => path.qself.is_none() && is_constant_path(&path.path),
        Expr::Call(ExprCall { func, args, .. }) => {
            !args.is_empty()
                && args.iter().all(is_constant)
                && (is_constructor(func) || CONSTANT_CALLS.iter().any(|call| path_is(func, call)))
        }
        _ => false,
    }
}

fn is_number(lit: &Lit) -> bool {
    matches!(lit, Lit::Int(_) | Lit::Float(_))
}

/// A `const` or `static` named in upper case, or a unit struct or enum
/// variant named in upper camel case behind its type or module.
fn is_constant_path(path: &syn::Path) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    let name = last.ident.to_string();
    is_constant_name(&name)
        || (path.segments.len() > 1 && name.starts_with(|c: char| c.is_ascii_uppercase()))
}

/// An upper-case name with a letter in it: a `const` or a `static`.
fn is_constant_name(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A path whose last segment is in upper camel case: a tuple struct or an
/// enum variant, whose call builds a value from its arguments.
fn is_constructor(func: &Expr) -> bool {
    let Expr::Path(path) = func else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            name.starts_with(|c: char| c.is_ascii_uppercase()) && !is_constant_name(&name)
        })
}

/// Whether `func` is a path ending in `segments`.
fn path_is(func: &Expr, segments: &[&str]) -> bool {
    let Expr::Path(path) = func else {
        return false;
    };
    if path.qself.is_some() || path.path.segments.len() < segments.len() {
        return false;
    }
    path.path
        .segments
        .iter()
        .rev()
        .zip(segments.iter().rev())
        .all(|(segment, name)| segment.ident == name && segment.arguments.is_none())
}

#[cfg(test)]
#[path = "tests/constant_modifiers_tests.rs"]
mod tests;

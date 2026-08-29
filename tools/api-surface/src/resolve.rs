//! Computes, for one crate's source tree, exactly which items are
//! "reachable via an unbroken chain of plain `pub` from the crate root,
//! excluding `#[doc(hidden)]` and `cfg(test)`" -- the same definition of
//! public API surface `cargo doc` uses, derived directly from the syntax
//! tree instead of from a compiled crate.
use std::{
    cell::Cell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use quote::ToTokens;
use syn::{Attribute, Fields, ImplItem, Item, TraitItem, Type, UseTree, Visibility};

/// Longest rendered signature kept on a [`MemberItem`] or [`NamedItem`];
/// longer renderings are truncated with a trailing marker so one oversized
/// enum or struct cannot dominate a generated report.
pub const MAX_SIGNATURE_LEN: usize = 400;

/// Truncates `s` to [`MAX_SIGNATURE_LEN`] bytes at a UTF-8 char boundary,
/// appending a trailing marker when truncation happened.
pub fn truncate_signature(mut s: String) -> String {
    if s.len() > MAX_SIGNATURE_LEN {
        let mut cut = MAX_SIGNATURE_LEN;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str(" ...");
    }
    s
}

fn render_tokens(tokens: impl ToTokens) -> String {
    truncate_signature(tokens.to_token_stream().to_string())
}

fn is_composable_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("composable"))
}

/// A module path as a sequence of segment names from the crate root; the
/// crate root itself is the empty path.
pub type ModPath = Vec<String>;

/// A field, enum variant, trait item, or impl item: everything whose own
/// reachability is derived from its parent [`NamedItem`] rather than
/// computed independently.
#[derive(Debug)]
pub struct MemberItem {
    pub is_pub: bool,
    pub excluded: bool,
    pub reachable: Cell<bool>,
    /// Field, variant, trait item, or impl item name; the field's index
    /// (as a string) for an anonymous tuple-struct field.
    pub name: String,
    /// Best-effort rendered signature: a field or const's type, or a
    /// method's full `fn` signature without a body.
    pub signature: String,
}

/// A `fn`, `struct`, `enum`, `union`, `trait`, `trait_alias`, `const`,
/// `static`, `type`, or `macro_rules!` item declared directly inside a
/// module (as opposed to inside an `impl` block; see [`DeferredImpl`]).
#[derive(Debug)]
pub struct NamedItem {
    pub ident: String,
    pub module_path: ModPath,
    pub is_pub: bool,
    pub doc_hidden: bool,
    pub excluded: bool,
    /// Set for a `#[macro_export]` `macro_rules!`, which is reachable at
    /// the crate root regardless of the module it is declared in.
    pub always_reachable: bool,
    pub reachable: Cell<bool>,
    /// `fn`, `struct`, `enum`, `union`, `trait`, `trait_alias`, `const`,
    /// `static`, `type`, or `macro`.
    pub kind: String,
    /// Best-effort rendered signature; a function's body is always elided,
    /// other kinds keep their declaration (fields, variants, trait items)
    /// up to [`MAX_SIGNATURE_LEN`].
    pub signature: String,
    /// Whether a `#[composable]` attribute is present on this item.
    pub composable: bool,
    /// A trait's methods/consts/types, a struct/union's fields, or an
    /// enum's variants (and the fields nested inside a struct- or
    /// tuple-style variant).
    pub sub_items: Vec<MemberItem>,
}

/// An `impl` block, resolved against its `Self` type's reachability once
/// every named item's own reachability is known (hence "deferred": an
/// `impl` can appear before the type it targets in source order).
#[derive(Debug)]
pub struct DeferredImpl {
    pub module_path: ModPath,
    pub self_type_ident: Option<String>,
    pub excluded: bool,
    pub members: Vec<MemberItem>,
    pub reachable: Cell<bool>,
}

/// One `pub use` target: either a single renamed/plain import
/// (`target_ident` set) or a glob import (`target_ident` `None`).
#[derive(Debug)]
pub struct UseEdge {
    pub target_module: ModPath,
    pub target_ident: Option<String>,
}

/// One source file's worth of directly-declared items: its own
/// reachability, its `pub use` re-export edges, and every `use` item (for
/// re-export bookkeeping).
#[derive(Debug)]
pub struct ModuleNode {
    pub is_pub: bool,
    pub excluded: bool,
    pub reachable: Cell<bool>,
    pub pub_use_edges: Vec<UseEdge>,
    pub use_items: Vec<MemberItem>,
}

/// Everything collected while loading one crate: every module, every
/// directly-declared named item, every deferred `impl`, plus any loader
/// warnings.
#[derive(Default)]
pub struct CrateTree {
    pub modules: HashMap<ModPath, ModuleNode>,
    pub named_items: Vec<NamedItem>,
    pub deferred_impls: Vec<DeferredImpl>,
    pub warnings: Vec<String>,
}

fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = a.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

fn has_doc_hidden(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("doc") {
            return false;
        }
        let mut found = false;
        let _ = a.parse_nested_meta(|meta| {
            if meta.path.is_ident("hidden") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

fn has_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test") || a.path().is_ident("tokio") || a.path().is_ident("bench")
    })
}

fn vis_is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn module_dir(file: &Path) -> PathBuf {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    if name == "lib.rs" || name == "main.rs" || name == "mod.rs" {
        parent.to_path_buf()
    } else {
        parent.join(file.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
    }
}

/// The directory this file's own plain `mod x;` children are searched in.
/// Ordinarily that is derived from the file's own name (`module_dir`), but
/// the caller forces a different directory in two cases: a file reached
/// through `#[path = "..."]` keeps the *file's own* parent directory
/// rather than a subdirectory named after the arbitrarily-chosen `#[path]`
/// target, and an inline `mod x { .. }` block nests its children one level
/// under the *enclosing* file's own directory, named after `x`, regardless
/// of that file's name.
fn children_dir_for(file: &Path, forced_dir_for_children: Option<PathBuf>) -> PathBuf {
    forced_dir_for_children.unwrap_or_else(|| module_dir(file))
}

fn path_attr(attrs: &[Attribute]) -> Option<String> {
    for a in attrs {
        if a.path().is_ident("path")
            && let syn::Meta::NameValue(nv) = &a.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Some(s.value());
        }
    }
    None
}

fn resolve_mod_file(
    dir_for_children: &Path,
    name: &str,
    attrs: &[Attribute],
    declaring_file: &Path,
) -> Result<PathBuf> {
    if let Some(p) = path_attr(attrs) {
        let base = declaring_file.parent().unwrap_or_else(|| Path::new("."));
        return Ok(base.join(p));
    }
    let as_leaf = dir_for_children.join(format!("{name}.rs"));
    let as_dir = dir_for_children.join(name).join("mod.rs");
    match (as_leaf.exists(), as_dir.exists()) {
        (true, false) => Ok(as_leaf),
        (false, true) => Ok(as_dir),
        (true, true) => anyhow::bail!(
            "ambiguous module `{name}`: both {} and {} exist",
            as_leaf.display(),
            as_dir.display()
        ),
        (false, false) => anyhow::bail!(
            "cannot find file for `mod {name};` declared in {} (looked for {} and {})",
            declaring_file.display(),
            as_leaf.display(),
            as_dir.display()
        ),
    }
}

fn self_type_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => self_type_ident(&r.elem),
        Type::Group(g) => self_type_ident(&g.elem),
        Type::Paren(p) => self_type_ident(&p.elem),
        _ => None,
    }
}

fn collect_use_edges(
    current: &ModPath,
    tree: &UseTree,
    prefix: &mut ModPath,
    edges: &mut Vec<UseEdge>,
    warnings: &mut Vec<String>,
) {
    match tree {
        UseTree::Path(p) => {
            let seg = p.ident.to_string();
            let mut next_prefix = prefix.clone();
            match seg.as_str() {
                "crate" => next_prefix = vec![],
                "self" => next_prefix = current.clone(),
                "super" => {
                    let mut base = if prefix.is_empty() {
                        current.clone()
                    } else {
                        prefix.clone()
                    };
                    if base.pop().is_none() {
                        warnings.push(format!(
                            "`super` past crate root while resolving use in {current:?}"
                        ));
                    }
                    next_prefix = base;
                }
                _ => {
                    if prefix.is_empty() {
                        next_prefix = current.clone();
                        next_prefix.push(seg);
                    } else {
                        next_prefix.push(seg);
                    }
                }
            }
            collect_use_edges(current, &p.tree, &mut next_prefix, edges, warnings);
        }
        UseTree::Name(n) => {
            let ident = n.ident.to_string();
            if ident == "self" {
                edges.push(UseEdge {
                    target_module: prefix.clone(),
                    target_ident: None,
                });
            } else {
                edges.push(UseEdge {
                    target_module: prefix.clone(),
                    target_ident: Some(ident),
                });
            }
        }
        UseTree::Rename(r) => {
            edges.push(UseEdge {
                target_module: prefix.clone(),
                target_ident: Some(r.ident.to_string()),
            });
        }
        UseTree::Glob(_) => {
            edges.push(UseEdge {
                target_module: prefix.clone(),
                target_ident: None,
            });
        }
        UseTree::Group(g) => {
            for item in &g.items {
                collect_use_edges(current, item, &mut prefix.clone(), edges, warnings);
            }
        }
    }
}

fn fields_to_members(fields: &Fields, force_pub: bool, excluded: bool) -> Vec<MemberItem> {
    match fields {
        Fields::Named(n) => n
            .named
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let name = f
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| i.to_string());
                member_from_attrs(
                    &f.attrs,
                    force_pub || vis_is_pub(&f.vis),
                    excluded,
                    name,
                    render_tokens(&f.ty),
                )
            })
            .collect(),
        Fields::Unnamed(n) => n
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, f)| {
                member_from_attrs(
                    &f.attrs,
                    force_pub || vis_is_pub(&f.vis),
                    excluded,
                    i.to_string(),
                    render_tokens(&f.ty),
                )
            })
            .collect(),
        Fields::Unit => vec![],
    }
}

/// An enum variant's own entry, plus (for a struct- or tuple-style variant)
/// its fields. Rust gives variant fields no visibility keyword of their
/// own -- they are exactly as visible as the variant, which is exactly as
/// visible as the enum -- so every entry here is forced reachable-if-parent.
fn variant_to_members(v: &syn::Variant, excluded: bool) -> Vec<MemberItem> {
    let signature = render_tokens(&v.fields);
    let mut out = vec![member_from_attrs(
        &v.attrs,
        true,
        excluded,
        v.ident.to_string(),
        signature,
    )];
    out.extend(fields_to_members(&v.fields, true, excluded));
    out
}

fn member_from_attrs(
    attrs: &[Attribute],
    is_pub: bool,
    excluded: bool,
    name: String,
    signature: String,
) -> MemberItem {
    MemberItem {
        is_pub,
        excluded: excluded || has_cfg_test(attrs) || has_doc_hidden(attrs),
        reachable: Cell::new(false),
        name,
        signature,
    }
}

/// Parses one crate's source tree, following `mod` declarations to their
/// files, and collects every module, named item, and deferred `impl` --
/// without yet deciding what is reachable; call [`compute_reachability`]
/// once loading finishes.
pub struct Loader {
    pub tree: CrateTree,
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

impl Loader {
    pub fn new() -> Self {
        Loader {
            tree: CrateTree::default(),
        }
    }

    pub fn load_crate(&mut self, root_file: &Path) -> Result<()> {
        self.load_file_as_module(vec![], root_file.to_path_buf(), true, false, None)
    }

    fn load_file_as_module(
        &mut self,
        path: ModPath,
        file: PathBuf,
        is_pub: bool,
        excluded: bool,
        forced_dir_for_children: Option<PathBuf>,
    ) -> Result<()> {
        let src =
            fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        let ast = syn::parse_file(&src).with_context(|| format!("parsing {}", file.display()))?;
        self.process_items(
            path,
            file,
            &ast.items,
            is_pub,
            excluded,
            forced_dir_for_children,
        )
    }

    fn process_items(
        &mut self,
        path: ModPath,
        file: PathBuf,
        items: &[Item],
        is_pub: bool,
        excluded: bool,
        forced_dir_for_children: Option<PathBuf>,
    ) -> Result<()> {
        let mut node = ModuleNode {
            is_pub,
            excluded,
            reachable: Cell::new(false),
            pub_use_edges: vec![],
            use_items: vec![],
        };
        let dir_for_children = children_dir_for(&file, forced_dir_for_children);
        let mut file_children: Vec<(ModPath, PathBuf, bool, bool, Option<PathBuf>)> = vec![];

        for item in items {
            match item {
                Item::Mod(m) => {
                    let name = m.ident.to_string();
                    let child_is_pub = vis_is_pub(&m.vis);
                    let child_excluded =
                        excluded || has_cfg_test(&m.attrs) || has_doc_hidden(&m.attrs);
                    let mut child_path = path.clone();
                    child_path.push(name.clone());
                    if let Some((_, inner_items)) = &m.content {
                        self.process_items(
                            child_path,
                            file.clone(),
                            inner_items,
                            child_is_pub,
                            child_excluded,
                            Some(dir_for_children.join(&name)),
                        )?;
                    } else {
                        let child_file =
                            resolve_mod_file(&dir_for_children, &name, &m.attrs, &file)?;
                        let child_forced_dir = path_attr(&m.attrs).map(|_| {
                            child_file
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .to_path_buf()
                        });
                        file_children.push((
                            child_path,
                            child_file,
                            child_is_pub,
                            child_excluded,
                            child_forced_dir,
                        ));
                    }
                }
                Item::Use(u) => {
                    let use_is_pub = vis_is_pub(&u.vis);
                    if use_is_pub {
                        collect_use_edges(
                            &path,
                            &u.tree,
                            &mut vec![],
                            &mut node.pub_use_edges,
                            &mut self.tree.warnings,
                        );
                    }
                    let use_signature = render_tokens(&u.tree);
                    node.use_items.push(member_from_attrs(
                        &u.attrs,
                        use_is_pub,
                        excluded,
                        use_signature.clone(),
                        use_signature,
                    ));
                }
                Item::Impl(imp) => {
                    let self_ty = self_type_ident(&imp.self_ty);
                    let impl_excluded = excluded || has_cfg_test(&imp.attrs);
                    let mut members = vec![];
                    for member in &imp.items {
                        match member {
                            ImplItem::Fn(f) => members.push(member_from_attrs(
                                &f.attrs,
                                vis_is_pub(&f.vis),
                                impl_excluded,
                                f.sig.ident.to_string(),
                                render_tokens(&f.sig),
                            )),
                            ImplItem::Const(c) => members.push(member_from_attrs(
                                &c.attrs,
                                vis_is_pub(&c.vis),
                                impl_excluded,
                                c.ident.to_string(),
                                render_tokens(&c.ty),
                            )),
                            ImplItem::Type(t) => members.push(member_from_attrs(
                                &t.attrs,
                                vis_is_pub(&t.vis),
                                impl_excluded,
                                t.ident.to_string(),
                                render_tokens(&t.ty),
                            )),
                            _ => {}
                        }
                    }
                    self.tree.deferred_impls.push(DeferredImpl {
                        module_path: path.clone(),
                        self_type_ident: self_ty,
                        excluded: impl_excluded,
                        members,
                        reachable: Cell::new(false),
                    });
                }
                other => {
                    if let Some(ni) = self.build_named_item(other, &path, excluded) {
                        self.tree.named_items.push(ni);
                    }
                }
            }
        }

        self.tree.modules.insert(path.clone(), node);
        for (cpath, cfile, cpub, cexcl, cforced_dir) in file_children {
            self.load_file_as_module(cpath, cfile, cpub, cexcl, cforced_dir)?;
        }
        Ok(())
    }

    fn build_named_item(
        &mut self,
        item: &Item,
        path: &ModPath,
        module_excluded: bool,
    ) -> Option<NamedItem> {
        let (ident, vis, attrs, always_reachable, sub_items, kind, signature): (
            String,
            Option<&Visibility>,
            &[Attribute],
            bool,
            Vec<MemberItem>,
            &str,
            String,
        ) = match item {
            Item::Fn(f) => (
                f.sig.ident.to_string(),
                Some(&f.vis),
                &f.attrs,
                false,
                vec![],
                "fn",
                render_tokens(&f.sig),
            ),
            Item::Struct(s) => {
                let members = fields_to_members(&s.fields, false, module_excluded);
                (
                    s.ident.to_string(),
                    Some(&s.vis),
                    &s.attrs,
                    false,
                    members,
                    "struct",
                    render_tokens(&s.fields),
                )
            }
            Item::Enum(e) => {
                let members = e
                    .variants
                    .iter()
                    .flat_map(|v| variant_to_members(v, module_excluded || has_cfg_test(&v.attrs)))
                    .collect();
                let names: Vec<String> = e.variants.iter().map(|v| v.ident.to_string()).collect();
                (
                    e.ident.to_string(),
                    Some(&e.vis),
                    &e.attrs,
                    false,
                    members,
                    "enum",
                    truncate_signature(names.join(", ")),
                )
            }
            Item::Union(u) => {
                let members =
                    fields_to_members(&Fields::Named(u.fields.clone()), false, module_excluded);
                (
                    u.ident.to_string(),
                    Some(&u.vis),
                    &u.attrs,
                    false,
                    members,
                    "union",
                    render_tokens(&u.fields),
                )
            }
            Item::Const(c) => (
                c.ident.to_string(),
                Some(&c.vis),
                &c.attrs,
                false,
                vec![],
                "const",
                render_tokens(&c.ty),
            ),
            Item::Static(s) => (
                s.ident.to_string(),
                Some(&s.vis),
                &s.attrs,
                false,
                vec![],
                "static",
                render_tokens(&s.ty),
            ),
            Item::Type(t) => (
                t.ident.to_string(),
                Some(&t.vis),
                &t.attrs,
                false,
                vec![],
                "type",
                render_tokens(&t.ty),
            ),
            Item::TraitAlias(t) => (
                t.ident.to_string(),
                Some(&t.vis),
                &t.attrs,
                false,
                vec![],
                "trait_alias",
                render_tokens(&t.bounds),
            ),
            Item::Trait(t) => {
                let members = t
                    .items
                    .iter()
                    .filter_map(|ti| match ti {
                        TraitItem::Fn(f) => Some(member_from_attrs(
                            &f.attrs,
                            true,
                            module_excluded,
                            f.sig.ident.to_string(),
                            render_tokens(&f.sig),
                        )),
                        TraitItem::Const(c) => Some(member_from_attrs(
                            &c.attrs,
                            true,
                            module_excluded,
                            c.ident.to_string(),
                            render_tokens(&c.ty),
                        )),
                        TraitItem::Type(ty) => Some(member_from_attrs(
                            &ty.attrs,
                            true,
                            module_excluded,
                            ty.ident.to_string(),
                            render_tokens(&ty.bounds),
                        )),
                        _ => None,
                    })
                    .collect();
                (
                    t.ident.to_string(),
                    Some(&t.vis),
                    &t.attrs,
                    false,
                    members,
                    "trait",
                    render_tokens(&t.supertraits),
                )
            }
            Item::Macro(m) => {
                let ident = m.ident.as_ref()?.to_string();
                let exported = m.attrs.iter().any(|a| a.path().is_ident("macro_export"));
                (
                    ident,
                    None,
                    &m.attrs,
                    exported,
                    vec![],
                    "macro",
                    String::new(),
                )
            }
            _ => return None,
        };
        let is_pub_val = vis.map(vis_is_pub).unwrap_or(false);
        let doc_hidden = has_doc_hidden(attrs);
        let excluded = module_excluded || has_cfg_test(attrs) || has_test_attr(attrs);
        let composable = is_composable_attr(attrs);
        Some(NamedItem {
            ident,
            module_path: path.clone(),
            is_pub: is_pub_val,
            doc_hidden,
            excluded,
            always_reachable,
            reachable: Cell::new(false),
            sub_items,
            kind: kind.to_string(),
            signature,
            composable,
        })
    }
}

/// Phase B + C + D: propagate reachability from the crate root through
/// `pub` module chains, then through `pub use` re-exports (to a fixpoint,
/// since a re-export can itself point at something only reachable through
/// another re-export), then resolve deferred `impl` blocks against their
/// now-final `Self` type reachability.
pub fn compute_reachability(tree: &CrateTree) {
    let mut module_paths: Vec<&ModPath> = tree.modules.keys().collect();
    module_paths.sort_by_key(|p| p.len());
    for path in &module_paths {
        let node = &tree.modules[*path];
        let parent_reachable = if path.is_empty() {
            true
        } else {
            let mut parent = (*path).clone();
            parent.pop();
            tree.modules
                .get(&parent)
                .map(|n| n.reachable.get())
                .unwrap_or(false)
        };
        let reach = parent_reachable && node.is_pub && !node.excluded;
        let reach = reach || path.is_empty();
        node.reachable.set(reach);
        for u in &node.use_items {
            u.reachable.set(reach && u.is_pub && !u.excluded);
        }
    }

    let item_index: HashMap<(ModPath, String), usize> = tree
        .named_items
        .iter()
        .enumerate()
        .map(|(i, it)| ((it.module_path.clone(), it.ident.clone()), i))
        .collect();

    for it in &tree.named_items {
        let module_reach = tree
            .modules
            .get(&it.module_path)
            .map(|n| n.reachable.get())
            .unwrap_or(false);
        let reach =
            it.always_reachable || (module_reach && it.is_pub && !it.doc_hidden && !it.excluded);
        it.reachable.set(reach);
    }

    let mut changed = true;
    let mut iterations = 0;
    while changed && iterations < 8 {
        changed = false;
        iterations += 1;
        for (path, node) in &tree.modules {
            if !node.reachable.get() {
                continue;
            }
            for edge in &node.pub_use_edges {
                match &edge.target_ident {
                    Some(name) => {
                        if let Some(&idx) =
                            item_index.get(&(edge.target_module.clone(), name.clone()))
                        {
                            let it = &tree.named_items[idx];
                            if !it.reachable.get() && !it.doc_hidden && !it.excluded {
                                it.reachable.set(true);
                                changed = true;
                            }
                        } else if let Some(target_mod) = tree.modules.get(&edge.target_module) {
                            if !target_mod.reachable.get() {
                                target_mod.reachable.set(true);
                                changed = true;
                            }
                        } else {
                            let _ = path;
                        }
                    }
                    None => {
                        for it in tree
                            .named_items
                            .iter()
                            .filter(|i| i.module_path == edge.target_module)
                        {
                            if !it.reachable.get() && !it.doc_hidden && !it.excluded {
                                it.reachable.set(true);
                                changed = true;
                            }
                        }
                        if let Some(target_mod) = tree.modules.get(&edge.target_module)
                            && !target_mod.reachable.get()
                        {
                            target_mod.reachable.set(true);
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    for imp in &tree.deferred_impls {
        let reach = match &imp.self_type_ident {
            Some(name) => {
                let same_module = tree
                    .named_items
                    .iter()
                    .find(|it| it.module_path == imp.module_path && &it.ident == name);
                if let Some(it) = same_module {
                    it.reachable.get()
                } else {
                    let matches: Vec<&NamedItem> = tree
                        .named_items
                        .iter()
                        .filter(|it| &it.ident == name)
                        .collect();
                    if matches.len() == 1 {
                        matches[0].reachable.get()
                    } else {
                        tree.modules
                            .get(&imp.module_path)
                            .map(|n| n.reachable.get())
                            .unwrap_or(false)
                    }
                }
            }
            None => tree
                .modules
                .get(&imp.module_path)
                .map(|n| n.reachable.get())
                .unwrap_or(false),
        };
        let reach = reach && !imp.excluded;
        imp.reachable.set(reach);
        for m in &imp.members {
            m.reachable.set(reach && !m.excluded && m.is_pub);
        }
    }

    for it in &tree.named_items {
        for m in &it.sub_items {
            m.reachable
                .set(it.reachable.get() && !m.excluded && m.is_pub);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_file(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    fn tmp_crate(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "cranpose_api_surface_test_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn public_fn_direct_is_reachable() {
        let dir = tmp_crate("public_fn");
        let root = write_file(&dir, "lib.rs", "pub fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(it.reachable.get());
        assert_eq!(it.kind, "fn");
    }

    #[test]
    fn private_fn_is_not_reachable() {
        let dir = tmp_crate("private_fn");
        let root = write_file(&dir, "lib.rs", "fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(!it.reachable.get());
    }

    #[test]
    fn pub_fn_inside_private_mod_is_not_reachable() {
        let dir = tmp_crate("private_in_public");
        let root = write_file(&dir, "lib.rs", "mod inner { pub fn a() {} }\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(!it.reachable.get());
    }

    #[test]
    fn pub_use_reexport_of_private_module_item_is_reachable() {
        let dir = tmp_crate("reexport");
        let root = write_file(
            &dir,
            "lib.rs",
            "mod inner { pub struct Widget; }\npub use inner::Widget;\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert_eq!(it.ident, "Widget");
        assert!(it.reachable.get());
        assert_eq!(it.kind, "struct");
    }

    #[test]
    fn glob_reexport_of_private_module_marks_all_items_reachable() {
        let dir = tmp_crate("glob_reexport");
        let root = write_file(
            &dir,
            "lib.rs",
            "mod inner { pub struct A; pub struct B; }\npub use inner::*;\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        assert!(loader.tree.named_items.iter().all(|it| it.reachable.get()));
    }

    #[test]
    fn cfg_test_module_is_never_reachable_even_if_pub() {
        let dir = tmp_crate("cfg_test");
        let root = write_file(
            &dir,
            "lib.rs",
            "#[cfg(test)]\npub mod tests { pub fn a() {} }\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(!it.reachable.get());
    }

    #[test]
    fn doc_hidden_item_is_not_reachable() {
        let dir = tmp_crate("doc_hidden");
        let root = write_file(&dir, "lib.rs", "#[doc(hidden)]\npub fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(!it.reachable.get());
    }

    #[test]
    fn composable_attribute_is_detected() {
        let dir = tmp_crate("composable_attr");
        let root = write_file(
            &dir,
            "lib.rs",
            "#[composable]\npub fn Box() {}\npub fn plain() {}\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let by_ident = |name: &str| {
            loader
                .tree
                .named_items
                .iter()
                .find(|it| it.ident == name)
                .unwrap()
        };
        assert!(by_ident("Box").composable);
        assert!(!by_ident("plain").composable);
    }

    #[test]
    fn impl_method_on_reexported_private_type_is_reachable() {
        let dir = tmp_crate("impl_reexport");
        let root = write_file(
            &dir,
            "lib.rs",
            "mod inner {\npub struct Widget;\nimpl Widget { pub fn new() -> Self { Widget } } }\npub use inner::Widget;\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let imp = &loader.tree.deferred_impls[0];
        assert!(imp.reachable.get());
        assert!(imp.members[0].reachable.get());
        assert_eq!(imp.members[0].name, "new");
        assert!(imp.members[0].signature.contains("fn new"));
    }

    #[test]
    fn impl_method_on_private_non_reexported_type_is_not_reachable() {
        let dir = tmp_crate("impl_private");
        let root = write_file(
            &dir,
            "lib.rs",
            "struct Widget;\nimpl Widget { pub fn new() -> Self { Widget } }\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let imp = &loader.tree.deferred_impls[0];
        assert!(!imp.reachable.get());
    }

    #[test]
    fn trait_method_reachable_when_trait_is_public() {
        let dir = tmp_crate("trait_method");
        let root = write_file(&dir, "lib.rs", "pub trait Foo { fn bar(&self); }\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let tr = &loader.tree.named_items[0];
        assert!(tr.reachable.get());
        assert!(tr.sub_items[0].reachable.get());
        assert_eq!(tr.sub_items[0].name, "bar");
    }

    #[test]
    fn pub_struct_field_name_and_type_are_kept_private_field_is_not_reachable() {
        let dir = tmp_crate("struct_fields");
        let root = write_file(
            &dir,
            "lib.rs",
            "pub struct Foo {\npub a: i32,\nb: i32,\n}\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let st = &loader.tree.named_items[0];
        assert!(st.reachable.get());
        assert!(st.sub_items[0].reachable.get());
        assert_eq!(st.sub_items[0].name, "a");
        assert_eq!(st.sub_items[0].signature, "i32");
        assert!(!st.sub_items[1].reachable.get());
    }

    #[test]
    fn struct_field_not_reachable_when_struct_itself_is_private() {
        let dir = tmp_crate("private_struct_fields");
        let root = write_file(&dir, "lib.rs", "struct Foo {\npub a: i32,\n}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let st = &loader.tree.named_items[0];
        assert!(!st.reachable.get());
        assert!(!st.sub_items[0].reachable.get());
    }

    #[test]
    fn enum_variant_reachable_when_enum_is_public_regardless_of_field_vis() {
        let dir = tmp_crate("enum_variants");
        let root = write_file(&dir, "lib.rs", "pub enum E {\nA,\nB { x: i32 },\n}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let en = &loader.tree.named_items[0];
        assert!(en.reachable.get());
        assert!(en.sub_items.iter().all(|m| m.reachable.get()));
        assert_eq!(en.sub_items.len(), 3);
        assert_eq!(en.signature, "A, B");
    }

    #[test]
    fn path_attr_redirects_submodule_file() {
        let dir = tmp_crate("path_attr");
        let root = write_file(&dir, "lib.rs", "#[path = \"custom/place.rs\"]\nmod sub;\n");
        write_file(&dir, "custom/place.rs", "pub fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        assert_eq!(loader.tree.named_items.len(), 1);
    }

    /// A `#[path]`-redirected file's own plain `mod child;` resolves next
    /// to the redirected file itself (`tests/child.rs`), not under a
    /// subdirectory named after the redirected file's own basename
    /// (`tests/main_tests/child.rs`) -- confirmed against real `rustc`
    /// before writing this resolver's rule, since this is the exact shape
    /// `#[cfg(test)] #[path = "tests/main_tests.rs"] mod tests;` takes in
    /// Cranpose's own `apps/desktop-demo`.
    #[test]
    fn path_attr_child_module_resolves_next_to_the_redirected_file() {
        let dir = tmp_crate("path_attr_child");
        let root = write_file(
            &dir,
            "lib.rs",
            "#[path = \"tests/main_tests.rs\"]\nmod tests;\n",
        );
        write_file(&dir, "tests/main_tests.rs", "mod child;\n");
        write_file(&dir, "tests/child.rs", "pub fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        assert_eq!(loader.tree.named_items.len(), 1);
    }

    /// An inline `mod outer { mod inner; }` nests `inner`'s file under a
    /// directory named after `outer`, relative to the *enclosing* file's
    /// own directory -- not relative to that enclosing file unchanged.
    /// Confirmed against real `rustc` before writing this resolver's rule.
    #[test]
    fn inline_mod_block_nests_child_file_under_its_own_name() {
        let dir = tmp_crate("inline_mod_nesting");
        let root = write_file(&dir, "lib.rs", "mod outer {\nmod inner;\n}\n");
        write_file(&dir, "outer/inner.rs", "pub fn a() {}\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        assert_eq!(loader.tree.named_items.len(), 1);
    }

    #[test]
    fn macro_export_is_always_reachable_regardless_of_module() {
        let dir = tmp_crate("macro_export");
        let root = write_file(
            &dir,
            "lib.rs",
            "mod inner { #[macro_export]\nmacro_rules! m { () => {} } }\n",
        );
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(it.reachable.get());
    }

    #[test]
    fn macro_rules_without_export_is_not_reachable() {
        let dir = tmp_crate("macro_no_export");
        let root = write_file(&dir, "lib.rs", "macro_rules! m { () => {} }\n");
        let mut loader = Loader::new();
        loader.load_crate(&root).unwrap();
        compute_reachability(&loader.tree);
        let it = &loader.tree.named_items[0];
        assert!(!it.reachable.get());
    }

    #[test]
    fn long_signature_is_truncated() {
        let long = "x".repeat(MAX_SIGNATURE_LEN + 50);
        let truncated = truncate_signature(long);
        assert!(truncated.ends_with(" ..."));
        assert!(truncated.len() <= MAX_SIGNATURE_LEN + 4);
    }

    #[test]
    fn short_signature_is_not_truncated() {
        assert_eq!(truncate_signature("i32".to_string()), "i32");
    }
}

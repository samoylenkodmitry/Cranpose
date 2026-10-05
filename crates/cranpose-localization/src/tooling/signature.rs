use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast::{self, Entry, Expression, InlineExpression, PatternElement};

/// Arguments required by a Fluent message, including references to other entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageSignature {
    /// All externally supplied variables used by the message.
    pub variables: BTreeSet<String>,
    /// Translator comments attached to the entry.
    pub comment: String,
}

#[derive(Default)]
struct Dependencies {
    variables: BTreeSet<String>,
    references: BTreeSet<String>,
    terms: BTreeSet<String>,
    functions: BTreeSet<String>,
}

/// Validates Fluent syntax and references and computes argument requirements.
pub fn catalog_signatures(source: &str) -> Result<BTreeMap<String, MessageSignature>, String> {
    let resource =
        fluent_syntax::parser::parse(source).map_err(|(_, errors)| format!("{errors:?}"))?;
    let mut entries = BTreeMap::new();
    let mut comments = BTreeMap::new();
    let mut identifiers = BTreeSet::from(["NUMBER"]);
    for entry in &resource.body {
        let name = match entry {
            Entry::Message(message) => Some(message.id.name),
            Entry::Term(term) => Some(term.id.name),
            _ => None,
        };
        if let Some(name) = name
            && !identifiers.insert(name)
        {
            return Err(format!("duplicate or reserved identifier `{name}`"));
        }
        match entry {
            Entry::Message(message) => {
                add_entry(
                    &mut entries,
                    message.id.name.to_owned(),
                    message.value.as_ref(),
                    &message.attributes,
                )?;
                if message.value.is_some() {
                    comments.insert(
                        message.id.name.to_owned(),
                        message
                            .comment
                            .as_ref()
                            .map(|comment| comment.content.join("\n"))
                            .unwrap_or_default(),
                    );
                }
            }
            Entry::Term(term) => add_entry(
                &mut entries,
                format!("-{}", term.id.name),
                Some(&term.value),
                &term.attributes,
            )?,
            _ => {}
        }
    }
    for name in entries.keys() {
        resolve(name, &entries, &mut BTreeSet::new())?;
    }
    comments
        .into_iter()
        .map(|(name, comment)| {
            let variables = resolve(&name, &entries, &mut BTreeSet::new())?;
            Ok((name, MessageSignature { variables, comment }))
        })
        .collect()
}

fn add_entry<'a>(
    entries: &mut BTreeMap<String, Dependencies>,
    name: String,
    value: Option<&ast::Pattern<&'a str>>,
    attributes: &[ast::Attribute<&'a str>],
) -> Result<(), String> {
    if let Some(pattern) = value {
        insert(entries, name.clone(), dependencies(pattern))?;
    }
    for attribute in attributes {
        insert(
            entries,
            format!("{name}.{}", attribute.id.name),
            dependencies(&attribute.value),
        )?;
    }
    Ok(())
}

fn insert(
    entries: &mut BTreeMap<String, Dependencies>,
    name: String,
    deps: Dependencies,
) -> Result<(), String> {
    if entries.insert(name.clone(), deps).is_some() {
        return Err(format!("duplicate entry `{name}`"));
    }
    Ok(())
}

fn dependencies(pattern: &ast::Pattern<&str>) -> Dependencies {
    let mut deps = Dependencies::default();
    walk_pattern(pattern, &mut deps);
    deps
}

fn walk_pattern(pattern: &ast::Pattern<&str>, deps: &mut Dependencies) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            walk_expression(expression, deps);
        }
    }
}

fn walk_expression(expression: &Expression<&str>, deps: &mut Dependencies) {
    match expression {
        Expression::Inline(inline) => walk_inline(inline, deps),
        Expression::Select { selector, variants } => {
            walk_inline(selector, deps);
            for variant in variants {
                walk_pattern(&variant.value, deps);
            }
        }
    }
}

fn walk_inline(inline: &InlineExpression<&str>, deps: &mut Dependencies) {
    match inline {
        InlineExpression::VariableReference { id } => {
            deps.variables.insert(id.name.to_owned());
        }
        InlineExpression::MessageReference { id, attribute } => {
            deps.references
                .insert(reference("", id.name, attribute.as_ref()));
        }
        InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            deps.terms
                .insert(reference("-", id.name, attribute.as_ref()));
            if let Some(arguments) = arguments {
                walk_arguments(arguments, deps);
            }
        }
        InlineExpression::FunctionReference { id, arguments } => {
            deps.functions.insert(id.name.to_owned());
            walk_arguments(arguments, deps);
        }
        InlineExpression::Placeable { expression } => walk_expression(expression, deps),
        _ => {}
    }
}

fn reference(prefix: &str, name: &str, attribute: Option<&ast::Identifier<&str>>) -> String {
    attribute.map_or_else(
        || format!("{prefix}{name}"),
        |attr| format!("{prefix}{name}.{}", attr.name),
    )
}

fn walk_arguments(arguments: &ast::CallArguments<&str>, deps: &mut Dependencies) {
    for argument in &arguments.positional {
        walk_inline(argument, deps);
    }
    for argument in &arguments.named {
        walk_inline(&argument.value, deps);
    }
}

fn resolve(
    name: &str,
    entries: &BTreeMap<String, Dependencies>,
    visiting: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    if !visiting.insert(name.to_owned()) {
        return Err(format!("cyclic message reference `{name}`"));
    }
    let deps = entries
        .get(name)
        .ok_or_else(|| format!("missing message reference `{name}`"))?;
    if let Some(function) = deps.functions.iter().find(|name| name.as_str() != "NUMBER") {
        return Err(format!("unsupported Fluent function `{function}`"));
    }
    let mut variables = deps.variables.clone();
    for reference in &deps.references {
        variables.extend(resolve(reference, entries, visiting)?);
    }
    for term in &deps.terms {
        resolve(term, entries, visiting)?;
    }
    visiting.remove(name);
    Ok(variables)
}

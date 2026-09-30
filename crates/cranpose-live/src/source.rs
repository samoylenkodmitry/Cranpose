use std::{collections::BTreeMap, rc::Rc};

use serde_json::Value;
use syn::{Expr, ExprCall, ExprClosure, ExprMethodCall, Stmt};

use crate::{Error, Expression, Node, Program, Registry, ValueType};

/// Translates a parameterless Rust UI function into a live document.
///
/// Supports registered calls, zero-argument content/action closures, scalar
/// literals, state-flow reads, `to_string()`, and explicit `key("id", || ...)`.
/// Unsupported Rust produces a diagnostic; no partial program is returned.
pub fn parse_source(registry: &Registry, source: &str, function: &str) -> Result<Program, Error> {
    if source.len() > registry.limits.source_bytes {
        return Err(Error::Source(
            "source exceeds the configured byte limit".into(),
        ));
    }
    let file = syn::parse_file(source).map_err(|error| Error::Source(error.to_string()))?;
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(item) if item.sig.ident == function => Some(item),
            _ => None,
        })
        .ok_or_else(|| Error::Source(format!("missing function '{function}'")))?;
    if !function.sig.inputs.is_empty()
        || !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
        || !matches!(function.sig.output, syn::ReturnType::Default)
    {
        return Err(Error::Source(
            "live entries need a synchronous, parameterless, non-generic unit signature".into(),
        ));
    }
    let mut nodes = block(registry, &function.block, "", 0)?;
    if nodes.len() != 1 {
        return Err(Error::Source(
            "a live entry needs exactly one root component".into(),
        ));
    }
    Ok(Program {
        root: nodes.remove(0),
    })
}

fn block(
    registry: &Registry,
    block: &syn::Block,
    parent: &str,
    depth: usize,
) -> Result<Vec<Rc<Node>>, Error> {
    let mut counts = BTreeMap::<String, usize>::new();
    block
        .stmts
        .iter()
        .map(|statement| {
            let Stmt::Expr(Expr::Call(call), _) = statement else {
                return Err(Error::Source(
                    "UI blocks currently contain component calls only".into(),
                ));
            };
            let component = path(&call.func)?;
            let count = counts.entry(component.clone()).or_default();
            let id = format!("{parent}/{component}:{count}");
            *count += 1;
            node(registry, call, id, depth)
        })
        .collect()
}

fn node(registry: &Registry, call: &ExprCall, id: String, depth: usize) -> Result<Rc<Node>, Error> {
    if depth > registry.limits.depth {
        return Err(Error::Source(
            "UI source exceeds the configured depth limit".into(),
        ));
    }
    let component = path(&call.func)?;
    if component == "key" {
        if call.args.len() != 2 {
            return Err(Error::Source(
                "key requires a string identity and one content closure".into(),
            ));
        }
        let Some(Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(id),
            ..
        })) = call.args.first()
        else {
            return Err(Error::Source(
                "key identity must be a string literal".into(),
            ));
        };
        let Some(Expr::Closure(content)) = call.args.last() else {
            return Err(Error::Source("key needs a content closure".into()));
        };
        let mut children = content_nodes(registry, content, &id.value(), depth + 1)?;
        if children.len() != 1 {
            return Err(Error::Source("key requires exactly one component".into()));
        }
        let mut child = children.remove(0);
        Rc::make_mut(&mut child).id = id.value();
        return Ok(child);
    }
    let registered = registry.component(&component)?;
    let schema = &registered.schema;
    if let Some(reason) = &schema.unavailable {
        return Err(Error::Source(format!("'{component}': {reason}")));
    }
    let expected = schema.parameters.len() + usize::from(schema.content.is_some());
    if call.args.len() != expected {
        return Err(Error::Source(format!(
            "'{component}' expects {expected} arguments"
        )));
    }
    let mut arguments = BTreeMap::new();
    for (value, parameter) in call.args.iter().zip(&schema.parameters) {
        let value = if parameter.value_type == ValueType::Action {
            action(value)?
        } else {
            expression(value)?
        };
        arguments.insert(parameter.name.clone(), value);
    }
    let children = if schema.content.is_some() {
        let Some(Expr::Closure(content)) = call.args.last() else {
            return Err(Error::Source(format!(
                "'{component}' needs a content closure"
            )));
        };
        content_nodes(registry, content, &id, depth + 1)?
    } else {
        Vec::new()
    };
    Ok(Rc::new(Node {
        id,
        component: schema.name.clone(),
        arguments,
        children,
    }))
}

fn content_nodes(
    registry: &Registry,
    closure: &ExprClosure,
    parent: &str,
    depth: usize,
) -> Result<Vec<Rc<Node>>, Error> {
    check_closure(closure)?;
    match &*closure.body {
        Expr::Block(body) => block(registry, &body.block, parent, depth),
        Expr::Call(call) => Ok(vec![node(
            registry,
            call,
            format!("{parent}/content:0"),
            depth,
        )?]),
        _ => Err(Error::Source(
            "content must be a component call or block".into(),
        )),
    }
}

fn path(expression: &Expr) -> Result<String, Error> {
    let Expr::Path(path) = expression else {
        return Err(Error::Source(
            "expected a named component or API instance".into(),
        ));
    };
    if path.qself.is_some()
        || path
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
    {
        return Err(Error::Source(
            "generic paths need a registered native adapter".into(),
        ));
    }
    Ok(path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::"))
}

fn expression(expr: &Expr) -> Result<Expression, Error> {
    if let Expr::MethodCall(call) = expr {
        check_method(call)?;
    }
    match expr {
        Expr::Lit(literal) => Ok(Expression::Literal {
            value: literal_value(&literal.lit)?,
        }),
        Expr::Paren(value) => expression(&value.expr),
        Expr::Unary(value) if matches!(value.op, syn::UnOp::Neg(_)) => negate(&value.expr),
        Expr::MethodCall(call) if call.args.is_empty() && call.method == "into" => {
            expression(&call.receiver)
        }
        Expr::MethodCall(call) if call.args.is_empty() && call.method == "to_string" => {
            Ok(Expression::Text {
                value: Box::new(expression(&call.receiver)?),
            })
        }
        Expr::MethodCall(call) => state(call),
        _ => Err(Error::Source(
            "supported arguments are literals, state reads and to_string()".into(),
        )),
    }
}

fn literal_value(literal: &syn::Lit) -> Result<Value, Error> {
    Ok(match literal {
        syn::Lit::Str(value) => Value::String(value.value()),
        syn::Lit::Bool(value) => Value::Bool(value.value),
        syn::Lit::Int(value) => Value::from(
            value
                .base10_parse::<i64>()
                .map_err(|e| Error::Source(e.to_string()))?,
        ),
        syn::Lit::Float(value) => serde_json::Number::from_f64(
            value
                .base10_parse::<f64>()
                .map_err(|e| Error::Source(e.to_string()))?,
        )
        .map(Value::Number)
        .ok_or_else(|| Error::Source("numbers must be finite".into()))?,
        _ => return Err(Error::Source("unsupported scalar literal".into())),
    })
}

fn negate(expr: &Expr) -> Result<Expression, Error> {
    let Expression::Literal { value } = expression(expr)? else {
        return Err(Error::Source(
            "negation currently requires a number literal".into(),
        ));
    };
    let value = if let Some(value) = value.as_i64() {
        Value::from(
            value
                .checked_neg()
                .ok_or_else(|| Error::Source("integer overflow".into()))?,
        )
    } else if let Some(value) = value.as_f64() {
        Value::from(-value)
    } else {
        return Err(Error::Source("negation requires a number".into()));
    };
    Ok(Expression::Literal { value })
}

fn state(call: &ExprMethodCall) -> Result<Expression, Error> {
    check_method(call)?;
    let mut call = call;
    if call.method == "get" && call.args.is_empty() {
        let Expr::MethodCall(collect) = &*call.receiver else {
            return Err(Error::Source("get() must follow collectAsState()".into()));
        };
        check_method(collect)?;
        if collect.method != "collectAsState" || !collect.args.is_empty() {
            return Err(Error::Source(
                "state reads use collectAsState().get()".into(),
            ));
        }
        let Expr::MethodCall(getter) = &*collect.receiver else {
            return Err(Error::Source("expected a view-model state getter".into()));
        };
        call = getter;
    }
    check_method(call)?;
    if !call.args.is_empty() {
        return Err(Error::Source("state getters take no arguments".into()));
    }
    Ok(Expression::State {
        api: path(&call.receiver)?,
        member: call.method.to_string(),
    })
}

fn action(expr: &Expr) -> Result<Expression, Error> {
    let Expr::Closure(closure) = expr else {
        return Err(Error::Source(
            "events use a zero-argument action closure".into(),
        ));
    };
    check_closure(closure)?;
    let body = match &*closure.body {
        Expr::Block(block) if block.block.stmts.len() == 1 => match &block.block.stmts[0] {
            Stmt::Expr(body, _) => body,
            _ => {
                return Err(Error::Source(
                    "an event must call a registered action".into(),
                ));
            }
        },
        body => body,
    };
    let Expr::MethodCall(call) = body else {
        return Err(Error::Source(
            "an event must call a registered action".into(),
        ));
    };
    check_method(call)?;
    Ok(Expression::Action {
        api: path(&call.receiver)?,
        member: call.method.to_string(),
        arguments: call.args.iter().map(expression).collect::<Result<_, _>>()?,
    })
}

fn check_closure(closure: &ExprClosure) -> Result<(), Error> {
    if !closure.inputs.is_empty()
        || closure.asyncness.is_some()
        || closure.constness.is_some()
        || closure.lifetimes.is_some()
        || !matches!(closure.output, syn::ReturnType::Default)
    {
        return Err(Error::Source(
            "live closures need a synchronous, zero-argument signature".into(),
        ));
    }
    Ok(())
}

fn check_method(call: &ExprMethodCall) -> Result<(), Error> {
    if call.turbofish.is_some() {
        return Err(Error::Source(
            "generic method calls need a native adapter".into(),
        ));
    }
    Ok(())
}

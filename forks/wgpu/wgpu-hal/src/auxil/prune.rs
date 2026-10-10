//! Branches a shader's pipeline constants decide, removed before GLSL is
//! written. Applying the constants turns each one into a literal, but a
//! condition such as `DRAWS_ARCS & (kind == ARC)` keeps its runtime side, so
//! the GLSL writer still writes both sides of the branch and the GL driver
//! compiles and links all of it. In Chrome on a Mali-G76 phone a solid fill
//! program of 31 KB vertex and 25 KB fragment GLSL linked in about 510 ms on
//! the page's only thread; without the branches its constants decide, 13 and
//! 12 KB, it linked in 30 ms.
//!
//! Each condition is folded as far as its decided operands allow, a call to
//! a function that returns a decided value takes that value, the side each
//! decided branch takes stands in its place, functions only the dropped
//! sides called are compacted away, and the module is validated again for
//! the writer.

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use core::ops::{BitAnd, BitOr, BitXor};

use naga::{
    valid::{Capabilities, ModuleInfo, ValidationFlags, Validator},
    Arena, BinaryOperator, Block, Constant, Expression, Function, Handle, Literal, Module,
    Statement, SwitchCase, UnaryOperator,
};

/// `module` without the branches its constants decide, with its info; the
/// module as it is when none is decided.
pub fn prune_decided_branches<'a>(
    module: Cow<'a, Module>,
    info: Cow<'a, ModuleInfo>,
) -> Result<(Cow<'a, Module>, Cow<'a, ModuleInfo>), String> {
    let decided: Vec<Option<Decided>> = {
        let mut lookup = Lookup {
            constants: &module.constants,
            globals: &module.global_expressions,
            returns: Vec::with_capacity(module.functions.len()),
        };
        functions(&module)
            .map(|function| {
                let values = lookup.decide(&function.expressions);
                lookup.returns.push(values.returned(&function.body));
                values.decides(&function.body).then_some(values)
            })
            .collect()
    };
    if decided.iter().all(Option::is_none) {
        return Ok((module, info));
    }
    let mut module = module.into_owned();
    let Module {
        functions,
        entry_points,
        ..
    } = &mut module;
    let pruned = functions
        .iter_mut()
        .map(|(_, function)| function)
        .chain(entry_points.iter_mut().map(|entry| &mut entry.function));
    for (function, values) in pruned.zip(&decided) {
        let Some(values) = values else {
            continue;
        };
        // The compaction keeps every named expression alive: a name left on
        // a dropped call's result would keep a call to a function it removes.
        let mut dropped = alloc::vec![false; function.expressions.len()];
        values.prune(&mut function.body, &mut dropped);
        function
            .named_expressions
            .retain(|handle, _| !dropped[handle.index()]);
    }
    naga::compact::compact(&mut module, naga::compact::KeepUnused::No);
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate_resolved_overrides(&module)
        .map_err(|error| format!("{error}"))?;
    Ok((Cow::Owned(module), Cow::Owned(info)))
}

/// The module's functions, then its entry points' functions.
fn functions(module: &Module) -> impl Iterator<Item = &Function> {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .chain(module.entry_points.iter().map(|entry| &entry.function))
}

/// Where a condition's value is found: the module's constants, and what each
/// function decided so far returns. A function comes before its callers.
struct Lookup<'m> {
    constants: &'m Arena<Constant>,
    globals: &'m Arena<Expression>,
    returns: Vec<Option<Value>>,
}

impl Lookup<'_> {
    /// The value of each of `expressions` the constants decide. An operand
    /// always comes before the expression using it.
    fn decide(&self, expressions: &Arena<Expression>) -> Decided {
        let mut values = Vec::with_capacity(expressions.len());
        for (_, expression) in expressions.iter() {
            let value = self.value(expression, &values);
            values.push(value);
        }
        Decided(values)
    }

    /// The value of `expression` when the constants decide it, given the
    /// values of the expressions `before` it.
    fn value(&self, expression: &Expression, before: &[Option<Value>]) -> Option<Value> {
        let operand = |handle| decided_at(before, handle);
        match *expression {
            Expression::Literal(literal) => Value::of(literal),
            Expression::Constant(constant) => match self.globals[self.constants[constant].init] {
                Expression::Literal(literal) => Value::of(literal),
                _ => None,
            },
            Expression::CallResult(function) => decided_at(&self.returns, function),
            Expression::Unary { op, expr } => operand(expr).and_then(|value| unary(op, value)),
            Expression::Binary { op, left, right } => binary(op, operand(left), operand(right)),
            Expression::Select {
                condition,
                accept,
                reject,
            } => match operand(condition) {
                Some(Value::Bool(taken)) => operand(if taken { accept } else { reject }),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The decided value at `handle`'s index of `values`.
fn decided_at<T>(values: &[Option<Value>], handle: Handle<T>) -> Option<Value> {
    values.get(handle.index()).copied().flatten()
}

/// A scalar the constants decide.
#[derive(Clone, Copy, PartialEq)]
enum Value {
    Bool(bool),
    U32(u32),
    I32(i32),
}

impl Value {
    fn of(literal: Literal) -> Option<Self> {
        match literal {
            Literal::Bool(value) => Some(Self::Bool(value)),
            Literal::U32(value) => Some(Self::U32(value)),
            Literal::I32(value) => Some(Self::I32(value)),
            _ => None,
        }
    }
}

fn unary(op: UnaryOperator, value: Value) -> Option<Value> {
    match (op, value) {
        (UnaryOperator::LogicalNot, Value::Bool(value)) => Some(Value::Bool(!value)),
        (UnaryOperator::BitwiseNot, Value::U32(value)) => Some(Value::U32(!value)),
        (UnaryOperator::BitwiseNot, Value::I32(value)) => Some(Value::I32(!value)),
        (UnaryOperator::Negate, Value::I32(value)) => Some(Value::I32(value.wrapping_neg())),
        _ => None,
    }
}

/// The value of a binary operation, when its decided operands fix it: both
/// are decided, or one alone settles a boolean `and` or `or`.
fn binary(op: BinaryOperator, left: Option<Value>, right: Option<Value>) -> Option<Value> {
    use BinaryOperator as Op;
    match (op, left, right) {
        (Op::LogicalAnd | Op::And, Some(Value::Bool(false)), _)
        | (Op::LogicalAnd | Op::And, _, Some(Value::Bool(false))) => Some(Value::Bool(false)),
        (Op::LogicalOr | Op::InclusiveOr, Some(Value::Bool(true)), _)
        | (Op::LogicalOr | Op::InclusiveOr, _, Some(Value::Bool(true))) => Some(Value::Bool(true)),
        (_, Some(left), Some(right)) => both(op, left, right),
        _ => None,
    }
}

fn both(op: BinaryOperator, left: Value, right: Value) -> Option<Value> {
    use BinaryOperator as Op;
    if let (Op::ShiftLeft | Op::ShiftRight, Value::U32(amount)) = (op, right) {
        let leftward = op == Op::ShiftLeft;
        return match left {
            Value::U32(value) if leftward => Some(Value::U32(value.wrapping_shl(amount))),
            Value::U32(value) => Some(Value::U32(value.wrapping_shr(amount))),
            Value::I32(value) if leftward => Some(Value::I32(value.wrapping_shl(amount))),
            Value::I32(value) => Some(Value::I32(value.wrapping_shr(amount))),
            Value::Bool(_) => None,
        };
    }
    match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => match op {
            Op::LogicalAnd | Op::And => Some(Value::Bool(left & right)),
            Op::LogicalOr | Op::InclusiveOr => Some(Value::Bool(left | right)),
            Op::Equal => Some(Value::Bool(left == right)),
            Op::NotEqual | Op::ExclusiveOr => Some(Value::Bool(left != right)),
            _ => None,
        },
        (Value::U32(left), Value::U32(right)) => integer(op, left, right, Value::U32),
        (Value::I32(left), Value::I32(right)) => integer(op, left, right, Value::I32),
        _ => None,
    }
}

/// A comparison or bitwise operation of two decided integers of one type.
fn integer<T>(op: BinaryOperator, left: T, right: T, wrap: fn(T) -> Value) -> Option<Value>
where
    T: Copy + PartialOrd + BitAnd<Output = T> + BitOr<Output = T> + BitXor<Output = T>,
{
    use BinaryOperator as Op;
    Some(match op {
        Op::Equal => Value::Bool(left == right),
        Op::NotEqual => Value::Bool(left != right),
        Op::Less => Value::Bool(left < right),
        Op::LessEqual => Value::Bool(left <= right),
        Op::Greater => Value::Bool(left > right),
        Op::GreaterEqual => Value::Bool(left >= right),
        Op::And => wrap(left & right),
        Op::InclusiveOr => wrap(left | right),
        Op::ExclusiveOr => wrap(left ^ right),
        _ => return None,
    })
}

/// The decided value of each expression of one function, by handle index.
struct Decided(Vec<Option<Value>>);

impl Decided {
    /// The branch `condition` decides.
    fn branch(&self, condition: Handle<Expression>) -> Option<bool> {
        match decided_at(&self.0, condition) {
            Some(Value::Bool(value)) => Some(value),
            _ => None,
        }
    }

    /// The value a function whose body is `body` always returns: its first
    /// statement other than an emit returns a decided value.
    fn returned(&self, body: &Block) -> Option<Value> {
        match *body.iter().find(|statement| !matches!(statement, Statement::Emit(_)))? {
            Statement::Return { value: Some(value) } => decided_at(&self.0, value),
            _ => None,
        }
    }

    /// Whether `block` holds a branch the constants decide.
    fn decides(&self, block: &Block) -> bool {
        block.iter().any(|statement| {
            let decided = match *statement {
                Statement::If { condition, .. } => self.branch(condition).is_some(),
                _ => false,
            };
            decided || nested(statement).any(|inner| self.decides(inner))
        })
    }

    /// Puts the side each decided branch in `block` takes in its place, and
    /// marks in `dropped` the expressions the other sides defined.
    fn prune(&self, block: &mut Block, dropped: &mut [bool]) {
        let statements = core::mem::take(block);
        for (mut statement, span) in statements.span_into_iter() {
            if let Statement::If {
                condition,
                ref mut accept,
                ref mut reject,
            } = statement
            {
                if let Some(value) = self.branch(condition) {
                    let (taken, other) = if value { (accept, reject) } else { (reject, accept) };
                    mark_defined(other, dropped);
                    let mut taken = core::mem::take(taken);
                    self.prune(&mut taken, dropped);
                    block.push(Statement::Block(taken), span);
                    continue;
                }
            }
            for inner in nested_mut(&mut statement) {
                self.prune(inner, dropped);
            }
            block.push(statement, span);
        }
    }
}

/// Marks in `defined` the expressions `block` emits and the results of the
/// calls it makes.
fn mark_defined(block: &Block, defined: &mut [bool]) {
    for statement in block.iter() {
        match *statement {
            Statement::Emit(ref range) => {
                for handle in range.clone() {
                    defined[handle.index()] = true;
                }
            }
            Statement::Call {
                result: Some(result),
                ..
            } => defined[result.index()] = true,
            _ => {}
        }
        for inner in nested(statement) {
            mark_defined(inner, defined);
        }
    }
}

/// The blocks `statement` holds.
fn nested(statement: &Statement) -> impl Iterator<Item = &Block> {
    let (pair, cases): ([Option<&Block>; 2], &[SwitchCase]) = match *statement {
        Statement::If {
            ref accept,
            ref reject,
            ..
        } => ([Some(accept), Some(reject)], &[]),
        Statement::Block(ref inner) => ([Some(inner), None], &[]),
        Statement::Loop {
            ref body,
            ref continuing,
            ..
        } => ([Some(body), Some(continuing)], &[]),
        Statement::Switch { ref cases, .. } => ([None, None], cases),
        _ => ([None, None], &[]),
    };
    pair.into_iter()
        .flatten()
        .chain(cases.iter().map(|case| &case.body))
}

/// The blocks `statement` holds, to change.
fn nested_mut(statement: &mut Statement) -> impl Iterator<Item = &mut Block> {
    let (pair, cases): ([Option<&mut Block>; 2], &mut [SwitchCase]) = match *statement {
        Statement::If {
            ref mut accept,
            ref mut reject,
            ..
        } => ([Some(accept), Some(reject)], &mut []),
        Statement::Block(ref mut inner) => ([Some(inner), None], &mut []),
        Statement::Loop {
            ref mut body,
            ref mut continuing,
            ..
        } => ([Some(body), Some(continuing)], &mut []),
        Statement::Switch { ref mut cases, .. } => ([None, None], cases),
        _ => ([None, None], &mut []),
    };
    pair.into_iter()
        .flatten()
        .chain(cases.iter_mut().map(|case| &mut case.body))
}

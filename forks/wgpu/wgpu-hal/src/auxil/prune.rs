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
//!
//! A constant often reaches its branch through a float: the liquid glass
//! shader reads each feature's strength as `fixed_or(value, 0.0, OFF)` and
//! tests `strength > 0.0`, often beside a runtime test in `a && b`, which
//! naga lowers into a local an `if` sets. So floats fold too, a call of a
//! small function takes the value its decided arguments fix, a local set
//! on both sides of an `if` takes the value both sides or its decided
//! condition give, and a product with a decided zero is zero (WGSL lets an
//! implementation assume no infinity or NaN). Each of the glass variants
//! the demo's Liquid UI tab draws fell from 80-82 KB of fragment GLSL to
//! 65-67 KB, and the Mate 20 X's Chrome spent 23% less GPU-process time on
//! them in a cold open (1746/1825 ms against 2263/2368 ms).

use alloc::{borrow::Cow, format, string::String, vec::Vec};
use core::ops::{BitAnd, BitOr, BitXor};

use naga::{
    valid::{Capabilities, FunctionInfo, ModuleInfo, ValidationFlags, Validator},
    Arena, BinaryOperator, Block, Expression, Function, Handle, Literal, MathFunction, Module,
    ScalarKind, Statement, SwitchCase, TypeInner, UnaryOperator,
};

/// `module` without the branches its constants decide, with its info; the
/// module as it is when none is decided.
#[inline(never)]
pub fn prune_decided_branches<'a>(
    module: Cow<'a, Module>,
    info: Cow<'a, ModuleInfo>,
) -> Result<(Cow<'a, Module>, Cow<'a, ModuleInfo>), String> {
    let decided: Vec<Option<Decided>> = {
        let mut lookup = Lookup {
            module: &module,
            info: &info,
            returns: Vec::with_capacity(module.functions.len()),
        };
        let infos = module
            .functions
            .iter()
            .map(|(handle, _)| &info[handle])
            .chain((0..module.entry_points.len()).map(|index| info.get_entry_point(index)));
        functions(&module)
            .zip(infos)
            .map(|(function, function_info)| {
                let values = lookup.decide(function, function_info, &[]);
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
    module: &'m Module,
    info: &'m ModuleInfo,
    returns: Vec<Option<Value>>,
}

/// The most expressions a function may have for a call's decided arguments
/// to be worked through it.
const SMALL_CALLEE: usize = 16;

impl Lookup<'_> {
    /// The value of each of `function`'s expressions the constants and its
    /// decided `arguments` decide. An operand always comes before the
    /// expression using it. Only a scalar takes a value.
    fn decide(
        &self,
        function: &Function,
        function_info: &FunctionInfo,
        arguments: &[Option<Value>],
    ) -> Decided {
        let body = BodyFacts::of(function);
        let mut values = Vec::with_capacity(function.expressions.len());
        for (handle, expression) in function.expressions.iter() {
            let scalar = matches!(
                function_info[handle].ty.inner_with(&self.module.types),
                TypeInner::Scalar(_)
            );
            let value = scalar
                .then(|| self.value(function, &body, handle, expression, &values, arguments))
                .flatten();
            values.push(value);
        }
        Decided(values)
    }

    /// The value of `expression` when the constants decide it, given the
    /// values of the expressions `before` it and of the function's
    /// `arguments`.
    fn value(
        &self,
        function: &Function,
        body: &BodyFacts,
        handle: Handle<Expression>,
        expression: &Expression,
        before: &[Option<Value>],
        arguments: &[Option<Value>],
    ) -> Option<Value> {
        let operand = |handle| decided_at(before, handle);
        match *expression {
            Expression::Literal(literal) => Value::of(literal),
            Expression::Constant(constant) => {
                match self.module.global_expressions[self.module.constants[constant].init] {
                    Expression::Literal(literal) => Value::of(literal),
                    _ => None,
                }
            }
            Expression::FunctionArgument(index) => {
                arguments.get(index as usize).copied().flatten()
            }
            Expression::CallResult(callee) => decided_at(&self.returns, callee)
                .or_else(|| self.call(callee, body.calls[handle.index()]?, before)),
            Expression::Load { pointer } => match function.expressions[pointer] {
                Expression::LocalVariable(variable) => {
                    body.temporaries[variable.index()]?.value(before)
                }
                _ => None,
            },
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
            Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                ..
            } => math(
                fun,
                operand(arg)?,
                arg1.and_then(operand),
                arg2.and_then(operand),
            ),
            Expression::As {
                expr,
                kind,
                convert: Some(4),
            } => operand(expr).and_then(|value| value.cast(kind)),
            _ => None,
        }
    }

    /// What a call of the small `callee` returns when the decided values of
    /// its `arguments` fix it.
    fn call(
        &self,
        callee: Handle<Function>,
        arguments: &[Handle<Expression>],
        before: &[Option<Value>],
    ) -> Option<Value> {
        let function = &self.module.functions[callee];
        if function.expressions.len() > SMALL_CALLEE {
            return None;
        }
        let values: Vec<Option<Value>> = arguments
            .iter()
            .map(|&argument| decided_at(before, argument))
            .collect();
        if values.iter().all(Option::is_none) {
            return None;
        }
        self.decide(function, &self.info[callee], &values)
            .returned(&function.body)
    }
}

/// What a function's statements tell about its expressions and locals.
struct BodyFacts<'f> {
    /// The arguments of the call each expression is the result of, by
    /// handle index.
    calls: Vec<Option<&'f [Handle<Expression>]>>,
    /// Each unnamed local that only an `if` sets, once on each side, by
    /// index: the way naga lowers a runtime `a && b` or `a || b`.
    temporaries: Vec<Option<Temporary>>,
}

impl<'f> BodyFacts<'f> {
    fn of(function: &'f Function) -> Self {
        let mut stores = alloc::vec![0u32; function.local_variables.len()];
        count_stores(&function.body, &function.expressions, &mut stores);
        let mut facts = Self {
            calls: alloc::vec![None; function.expressions.len()],
            temporaries: alloc::vec![None; function.local_variables.len()],
        };
        facts.note(&function.body, function, &stores);
        facts
    }

    fn note(&mut self, block: &'f Block, function: &Function, stores: &[u32]) {
        for statement in block.iter() {
            match *statement {
                Statement::Call {
                    result: Some(result),
                    ref arguments,
                    ..
                } => self.calls[result.index()] = Some(arguments),
                Statement::If {
                    condition,
                    ref accept,
                    ref reject,
                } => {
                    let stored = |block: &Block| match block.last() {
                        Some(&Statement::Store { pointer, value }) => {
                            match function.expressions[pointer] {
                                Expression::LocalVariable(variable) => Some((variable, value)),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let (Some((variable, accept)), Some((other, reject))) =
                        (stored(accept), stored(reject))
                    {
                        let local = &function.local_variables[variable];
                        if variable == other
                            && stores[variable.index()] == 2
                            && local.name.is_none()
                            && local.init.is_none()
                        {
                            self.temporaries[variable.index()] = Some(Temporary {
                                condition,
                                accept,
                                reject,
                            });
                        }
                    }
                }
                _ => {}
            }
            for inner in nested(statement) {
                self.note(inner, function, stores);
            }
        }
    }
}

/// Counts in `stores` the stores to each local, by index.
fn count_stores(block: &Block, expressions: &Arena<Expression>, stores: &mut [u32]) {
    for statement in block.iter() {
        if let Statement::Store { pointer, .. } = *statement {
            if let Expression::LocalVariable(variable) = expressions[pointer] {
                stores[variable.index()] += 1;
            }
        }
        for inner in nested(statement) {
            count_stores(inner, expressions, stores);
        }
    }
}

/// A local an `if` sets to `accept` or `reject` by its `condition`.
#[derive(Clone, Copy)]
struct Temporary {
    condition: Handle<Expression>,
    accept: Handle<Expression>,
    reject: Handle<Expression>,
}

impl Temporary {
    /// Its value after the `if`, when the decided values `before` fix it.
    fn value(self, before: &[Option<Value>]) -> Option<Value> {
        match decided_at(before, self.condition) {
            Some(Value::Bool(taken)) => decided_at(before, if taken { self.accept } else { self.reject }),
            _ => {
                let accept = decided_at(before, self.accept)?;
                (decided_at(before, self.reject) == Some(accept)).then_some(accept)
            }
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
    F32(f32),
}

impl Value {
    fn of(literal: Literal) -> Option<Self> {
        match literal {
            Literal::Bool(value) => Some(Self::Bool(value)),
            Literal::U32(value) => Some(Self::U32(value)),
            Literal::I32(value) => Some(Self::I32(value)),
            Literal::F32(value) => Some(Self::F32(value)),
            _ => None,
        }
    }

    /// The value converted to a 32-bit scalar of `kind`, as WGSL's
    /// conversions do.
    fn cast(self, kind: ScalarKind) -> Option<Self> {
        Some(match (self, kind) {
            (Self::F32(value), ScalarKind::Float) => Self::F32(value),
            (Self::F32(value), ScalarKind::Uint) => Self::U32(value as u32),
            (Self::F32(value), ScalarKind::Sint) => Self::I32(value as i32),
            (Self::U32(value), ScalarKind::Float) => Self::F32(value as f32),
            (Self::I32(value), ScalarKind::Float) => Self::F32(value as f32),
            (Self::Bool(value), ScalarKind::Float) => Self::F32(f32::from(u8::from(value))),
            (Self::Bool(value), ScalarKind::Uint) => Self::U32(u32::from(value)),
            (Self::Bool(value), ScalarKind::Sint) => Self::I32(i32::from(value)),
            (Self::U32(value), ScalarKind::Uint) => Self::U32(value),
            (Self::I32(value), ScalarKind::Sint) => Self::I32(value),
            _ => return None,
        })
    }
}

/// A float function of decided operands.
fn math(fun: MathFunction, arg: Value, arg1: Option<Value>, arg2: Option<Value>) -> Option<Value> {
    use MathFunction as Fun;
    let float = |value: Option<Value>| match value {
        Some(Value::F32(value)) => Some(value),
        _ => None,
    };
    let value = float(Some(arg))?;
    Some(Value::F32(match fun {
        Fun::Abs => value.abs(),
        Fun::Saturate => value.max(0.0).min(1.0),
        Fun::Max => value.max(float(arg1)?),
        Fun::Min => value.min(float(arg1)?),
        Fun::Clamp => value.max(float(arg1)?).min(float(arg2)?),
        _ => return None,
    }))
}

fn unary(op: UnaryOperator, value: Value) -> Option<Value> {
    match (op, value) {
        (UnaryOperator::LogicalNot, Value::Bool(value)) => Some(Value::Bool(!value)),
        (UnaryOperator::BitwiseNot, Value::U32(value)) => Some(Value::U32(!value)),
        (UnaryOperator::BitwiseNot, Value::I32(value)) => Some(Value::I32(!value)),
        (UnaryOperator::Negate, Value::I32(value)) => Some(Value::I32(value.wrapping_neg())),
        (UnaryOperator::Negate, Value::F32(value)) => Some(Value::F32(-value)),
        _ => None,
    }
}

/// The value of a binary operation, when its decided operands fix it: both
/// are decided, or one alone settles a boolean `and` or `or`, or is a zero
/// factor (GLSL ES keeps no NaN, so `0.0 * x` is `0.0`).
fn binary(op: BinaryOperator, left: Option<Value>, right: Option<Value>) -> Option<Value> {
    use BinaryOperator as Op;
    match (op, left, right) {
        (Op::LogicalAnd | Op::And, Some(Value::Bool(false)), _)
        | (Op::LogicalAnd | Op::And, _, Some(Value::Bool(false))) => Some(Value::Bool(false)),
        (Op::LogicalOr | Op::InclusiveOr, Some(Value::Bool(true)), _)
        | (Op::LogicalOr | Op::InclusiveOr, _, Some(Value::Bool(true))) => Some(Value::Bool(true)),
        (Op::Multiply, Some(Value::F32(zero)), _) | (Op::Multiply, _, Some(Value::F32(zero)))
            if zero == 0.0 =>
        {
            Some(Value::F32(0.0))
        }
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
            Value::Bool(_) | Value::F32(_) => None,
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
        (Value::F32(left), Value::F32(right)) => float(op, left, right),
        _ => None,
    }
}

/// An arithmetic operation or comparison of two decided floats.
fn float(op: BinaryOperator, left: f32, right: f32) -> Option<Value> {
    use BinaryOperator as Op;
    Some(match op {
        Op::Add => Value::F32(left + right),
        Op::Subtract => Value::F32(left - right),
        Op::Multiply => Value::F32(left * right),
        Op::Divide if right != 0.0 => Value::F32(left / right),
        Op::Equal => Value::Bool(left == right),
        Op::NotEqual => Value::Bool(left != right),
        Op::Less => Value::Bool(left < right),
        Op::LessEqual => Value::Bool(left <= right),
        Op::Greater => Value::Bool(left > right),
        Op::GreaterEqual => Value::Bool(left >= right),
        _ => return None,
    })
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

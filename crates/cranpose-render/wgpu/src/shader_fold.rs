//! A pipeline's stage specialized by its override constants and folded: a
//! branch its constants decide is replaced by the one it takes, and the
//! functions only the other called are dropped, so the driver compiles the
//! code the stage draws with and nothing else.
//!
//! The shape shader's variants guard each kind's code with conditions such
//! as `DRAWS_ARCS & (kind == ARC)`: once `DRAWS_ARCS` is a constant `false`,
//! naga keeps the branch, since its other side is known only at run time,
//! and every driver parsed and compiled the arc, line and gradient code of a
//! plain rect fill before folding it away. Folded, that fill's vertex stage
//! is 12.6 KB of SPIR-V instead of 36 KB, and on a Mate 20 X the gauntlet's
//! shape pipelines compiled in about 45 ms instead of 70.

use naga::{
    BinaryOperator as Op, Block, Expression, Function, Handle, Literal, MathFunction, Module,
    ScalarKind, Statement, UnaryOperator,
};

/// A value the constants decide.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Bool(bool),
    U32(u32),
    I32(i32),
}

impl Value {
    fn of(literal: &Literal) -> Option<Self> {
        match *literal {
            Literal::Bool(value) => Some(Self::Bool(value)),
            Literal::U32(value) => Some(Self::U32(value)),
            Literal::I32(value) => Some(Self::I32(value)),
            _ => None,
        }
    }

    fn int(self) -> Option<i64> {
        match self {
            Self::U32(value) => Some(i64::from(value)),
            Self::I32(value) => Some(i64::from(value)),
            Self::Bool(_) => None,
        }
    }

    /// `number` as this value's type, wrapping as the GPU does.
    fn like(self, number: i64) -> Self {
        match self {
            Self::U32(_) => Self::U32(number as u32),
            Self::I32(_) => Self::I32(number as i32),
            Self::Bool(_) => Self::Bool(number != 0),
        }
    }
}

/// The value one side gives a logical operator whatever the other holds.
fn decisive(op: Op, left: Option<Value>, right: Option<Value>) -> Option<Value> {
    let decides = |value| left == Some(Value::Bool(value)) || right == Some(Value::Bool(value));
    match op {
        Op::LogicalAnd | Op::And if decides(false) => Some(Value::Bool(false)),
        Op::LogicalOr | Op::InclusiveOr if decides(true) => Some(Value::Bool(true)),
        _ => None,
    }
}

fn logical(op: Op, a: bool, b: bool) -> Option<bool> {
    match op {
        Op::LogicalAnd | Op::And => Some(a & b),
        Op::LogicalOr | Op::InclusiveOr => Some(a | b),
        Op::ExclusiveOr | Op::NotEqual => Some(a ^ b),
        Op::Equal => Some(a == b),
        _ => None,
    }
}

fn compare(op: Op, a: i64, b: i64) -> Option<bool> {
    match op {
        Op::Equal => Some(a == b),
        Op::NotEqual => Some(a != b),
        Op::Less => Some(a < b),
        Op::LessEqual => Some(a <= b),
        Op::Greater => Some(a > b),
        Op::GreaterEqual => Some(a >= b),
        _ => None,
    }
}

fn arithmetic(op: Op, a: i64, b: i64) -> Option<i64> {
    match op {
        Op::And => Some(a & b),
        Op::InclusiveOr => Some(a | b),
        Op::ExclusiveOr => Some(a ^ b),
        Op::Add => Some(a.wrapping_add(b)),
        Op::Subtract => Some(a.wrapping_sub(b)),
        Op::Multiply => Some(a.wrapping_mul(b)),
        // A shift takes its amount modulo the width, as the GPU does.
        Op::ShiftLeft => Some(a << (b & 31)),
        Op::ShiftRight => Some(a >> (b & 31)),
        _ => None,
    }
}

fn binary(op: Op, left: Option<Value>, right: Option<Value>) -> Option<Value> {
    if let Some(value) = decisive(op, left, right) {
        return Some(value);
    }
    let (left, right) = (left?, right?);
    if let (Value::Bool(a), Value::Bool(b)) = (left, right) {
        return logical(op, a, b).map(Value::Bool);
    }
    let (a, b) = (left.int()?, right.int()?);
    compare(op, a, b)
        .map(Value::Bool)
        .or_else(|| arithmetic(op, a, b).map(|number| left.like(number)))
}

fn extreme(fun: MathFunction, a: Option<Value>, b: Option<Value>) -> Option<Value> {
    let (a, b) = (a?, b?);
    let (x, y) = (a.int()?, b.int()?);
    Some(a.like(if fun == MathFunction::Max {
        x.max(y)
    } else {
        x.min(y)
    }))
}

fn cast(value: Value, kind: ScalarKind) -> Option<Value> {
    let number = match value {
        Value::Bool(value) => i64::from(value),
        other => other.int()?,
    };
    match kind {
        ScalarKind::Uint => Some(Value::U32(number as u32)),
        ScalarKind::Sint => Some(Value::I32(number as i32)),
        ScalarKind::Bool => Some(Value::Bool(number != 0)),
        _ => None,
    }
}

/// The value of each of `function`'s expressions that constants decide,
/// by handle: an expression refers only to earlier ones.
fn decided(module: &Module, function: &Function) -> Vec<Option<Value>> {
    let mut values: Vec<Option<Value>> = Vec::with_capacity(function.expressions.len());
    for (_, expression) in function.expressions.iter() {
        let value = |handle: Handle<Expression>| values[handle.index()];
        let decided = match *expression {
            Expression::Literal(ref literal) => Value::of(literal),
            Expression::Constant(constant) => {
                match module.global_expressions[module.constants[constant].init] {
                    Expression::Literal(ref literal) => Value::of(literal),
                    _ => None,
                }
            }
            Expression::Unary {
                op: UnaryOperator::LogicalNot,
                expr,
            } => match value(expr) {
                Some(Value::Bool(value)) => Some(Value::Bool(!value)),
                _ => None,
            },
            Expression::Binary { op, left, right } => binary(op, value(left), value(right)),
            Expression::Select {
                condition,
                accept,
                reject,
            } => match value(condition) {
                Some(Value::Bool(true)) => value(accept),
                Some(Value::Bool(false)) => value(reject),
                _ => None,
            },
            Expression::As {
                expr,
                kind,
                convert: Some(_),
            } => value(expr).and_then(|value| cast(value, kind)),
            Expression::Math {
                fun: fun @ (MathFunction::Max | MathFunction::Min),
                arg,
                arg1: Some(arg1),
                ..
            } => extreme(fun, value(arg), value(arg1)),
            _ => None,
        };
        values.push(decided);
    }
    values
}

/// Whether nothing after `statement` in its block runs.
fn ends(statement: &Statement) -> bool {
    match statement {
        Statement::Return { .. } | Statement::Kill | Statement::Break | Statement::Continue => true,
        Statement::Block(block) => block.last().is_some_and(ends),
        _ => false,
    }
}

/// `block` with each branch its constants decide replaced by the side it
/// takes, and nothing after a statement that leaves it.
fn fold_block(block: Block, values: &[Option<Value>]) -> Block {
    let mut folded = Block::with_capacity(block.len());
    for (statement, span) in block.span_into_iter() {
        let statement = match statement {
            Statement::If {
                condition,
                accept,
                reject,
            } => match values[condition.index()] {
                Some(Value::Bool(taken)) => {
                    let kept = fold_block(if taken { accept } else { reject }, values);
                    if kept.is_empty() {
                        continue;
                    }
                    Statement::Block(kept)
                }
                _ => Statement::If {
                    condition,
                    accept: fold_block(accept, values),
                    reject: fold_block(reject, values),
                },
            },
            Statement::Block(inner) => Statement::Block(fold_block(inner, values)),
            Statement::Loop {
                body,
                continuing,
                break_if,
            } => Statement::Loop {
                body: fold_block(body, values),
                continuing: fold_block(continuing, values),
                break_if,
            },
            Statement::Switch { selector, cases } => Statement::Switch {
                selector,
                cases: cases
                    .into_iter()
                    .map(|case| naga::SwitchCase {
                        body: fold_block(case.body, values),
                        ..case
                    })
                    .collect(),
            },
            other => other,
        };
        let leaves = ends(&statement);
        folded.push(statement, span);
        if leaves {
            break;
        }
    }
    folded
}

/// Marks the expressions `block` still evaluates: its emitted ranges and the
/// results its statements produce.
fn mark_evaluated(block: &Block, evaluated: &mut [bool]) {
    for statement in block {
        match statement {
            Statement::Emit(range) => range
                .clone()
                .for_each(|handle| evaluated[handle.index()] = true),
            Statement::Block(inner) => mark_evaluated(inner, evaluated),
            Statement::If { accept, reject, .. } => {
                mark_evaluated(accept, evaluated);
                mark_evaluated(reject, evaluated);
            }
            Statement::Loop {
                body, continuing, ..
            } => {
                mark_evaluated(body, evaluated);
                mark_evaluated(continuing, evaluated);
            }
            Statement::Switch { cases, .. } => cases
                .iter()
                .for_each(|case| mark_evaluated(&case.body, evaluated)),
            Statement::Call {
                result: Some(result),
                ..
            }
            | Statement::Atomic {
                result: Some(result),
                ..
            }
            | Statement::WorkGroupUniformLoad { result, .. } => evaluated[result.index()] = true,
            _ => {}
        }
    }
}

/// Whether `expression` exists only where a statement evaluates it.
fn evaluated_in_place(expression: &Expression) -> bool {
    !matches!(
        expression,
        Expression::Literal(_)
            | Expression::Constant(_)
            | Expression::Override(_)
            | Expression::ZeroValue(_)
            | Expression::FunctionArgument(_)
            | Expression::GlobalVariable(_)
            | Expression::LocalVariable(_)
    )
}

fn fold_function(module: &Module, function: &mut Function) {
    let values = decided(module, function);
    let body = std::mem::take(&mut function.body);
    function.body = fold_block(body, &values);
    let mut evaluated = vec![false; function.expressions.len()];
    mark_evaluated(&function.body, &mut evaluated);
    // A name of an expression no branch left evaluates names nothing.
    let expressions = &function.expressions;
    function.named_expressions.retain(|handle, _| {
        evaluated[handle.index()] || !evaluated_in_place(&expressions[*handle])
    });
}

/// Folds `module`, whose overrides are constants already, and drops what no
/// entry point reaches any more.
pub(crate) fn fold(module: &mut Module) {
    // Folding reads only the module's constants: its functions and entry
    // points step out of it meanwhile.
    let mut functions = std::mem::take(&mut module.functions);
    for (_, function) in functions.iter_mut() {
        fold_function(module, function);
    }
    module.functions = functions;
    let mut entry_points = std::mem::take(&mut module.entry_points);
    for entry_point in &mut entry_points {
        fold_function(module, &mut entry_point.function);
    }
    module.entry_points = entry_points;
    naga::compact::compact(module, naga::compact::KeepUnused::No);
}

//! A folded stage flattened before the driver sees it: calls are inlined
//! into the entry point, private variables become its own, each struct
//! variable becomes one variable per member, a variable stored once ahead
//! of its loads hands them the stored value, and stores nothing reads are
//! dropped with the work that only fed them. Each rebuild also folds what
//! the constants decide once a callee sees the values its call passed.
//!
//! Drivers parse and compile every function a stage keeps and copy a struct
//! passed or returned by value at every call. The shape shader builds a
//! fifteen-member `VertexOutput` through four calls for a plain rect fill
//! that keeps four of its members. Flattened, that fill's stages are 7.2
//! and 4.5 KB of SPIR-V instead of 12.6 and 12.0. The twelve pipelines of
//! the gauntlet's first frame compiled one after another in 816 ms instead
//! of 1100 on an Apple M5 (Metal, cold). On a Mate 20 X's Mali-G76 (Vulkan,
//! cold) each shape pipeline compiled 2 to 7 ms sooner, about what the
//! passes took on its CPU. malioc gives the fragment stages the same
//! registers and cycles; vertex stages move by 0.05 cycles at most.
//!
//! A callee copied at many call sites stays a function: inlined, the blur
//! shader's sixteen kernel pairs, two sides each, grew its fragment stage
//! from 11 to 144 KB of SPIR-V (see [`INLINE_GROWTH`]).

use naga::{
    AddressSpace, Arena, Block, Expression, Function, Handle, LocalVariable, Module, Range, Span,
    Statement, SwitchCase, Type, TypeInner,
};

/// A `match` on `$expression` that hands `$visit` each operand, bound by
/// `$binding` (`ref` or `ref mut`): one list of every expression's operands
/// for reading them and for replacing them.
macro_rules! operands {
    ($expression:expr, $visit:ident, $($binding:tt)+) => {
        match *$expression {
            Expression::Compose { $($binding)+ components, .. } => {
                components.into_iter().for_each(&mut $visit);
            }
            Expression::Access { $($binding)+ base, $($binding)+ index } => {
                $visit(base);
                $visit(index);
            }
            Expression::AccessIndex { $($binding)+ base, .. } => $visit(base),
            Expression::Splat { $($binding)+ value, .. } => $visit(value),
            Expression::Swizzle { $($binding)+ vector, .. } => $visit(vector),
            Expression::Load { $($binding)+ pointer } => $visit(pointer),
            Expression::ImageSample {
                $($binding)+ image,
                $($binding)+ sampler,
                $($binding)+ coordinate,
                $($binding)+ array_index,
                $($binding)+ offset,
                $($binding)+ level,
                $($binding)+ depth_ref,
                ..
            } => {
                $visit(image);
                $visit(sampler);
                $visit(coordinate);
                array_index.into_iter().for_each(&mut $visit);
                offset.into_iter().for_each(&mut $visit);
                match level {
                    naga::SampleLevel::Exact(value) | naga::SampleLevel::Bias(value) => $visit(value),
                    naga::SampleLevel::Gradient { x, y } => {
                        $visit(x);
                        $visit(y);
                    }
                    naga::SampleLevel::Auto | naga::SampleLevel::Zero => {}
                }
                depth_ref.into_iter().for_each(&mut $visit);
            }
            Expression::ImageLoad {
                $($binding)+ image,
                $($binding)+ coordinate,
                $($binding)+ array_index,
                $($binding)+ sample,
                $($binding)+ level,
            } => {
                $visit(image);
                $visit(coordinate);
                array_index.into_iter().for_each(&mut $visit);
                sample.into_iter().for_each(&mut $visit);
                level.into_iter().for_each(&mut $visit);
            }
            Expression::ImageQuery { $($binding)+ image, $($binding)+ query } => {
                $visit(image);
                if let naga::ImageQuery::Size { level: Some(level) } = query {
                    $visit(level);
                }
            }
            Expression::Unary { $($binding)+ expr, .. }
            | Expression::Derivative { $($binding)+ expr, .. }
            | Expression::As { $($binding)+ expr, .. }
            | Expression::ArrayLength($($binding)+ expr) => $visit(expr),
            Expression::Binary { $($binding)+ left, $($binding)+ right, .. } => {
                $visit(left);
                $visit(right);
            }
            Expression::Select { $($binding)+ condition, $($binding)+ accept, $($binding)+ reject } => {
                $visit(condition);
                $visit(accept);
                $visit(reject);
            }
            Expression::Relational { $($binding)+ argument, .. } => $visit(argument),
            Expression::Math { $($binding)+ arg, $($binding)+ arg1, $($binding)+ arg2, $($binding)+ arg3, .. } => {
                $visit(arg);
                arg1.into_iter().for_each(&mut $visit);
                arg2.into_iter().for_each(&mut $visit);
                arg3.into_iter().for_each(&mut $visit);
            }
            Expression::RayQueryVertexPositions { $($binding)+ query, .. }
            | Expression::RayQueryGetIntersection { $($binding)+ query, .. } => $visit(query),
            Expression::CooperativeMultiplyAdd { $($binding)+ a, $($binding)+ b, $($binding)+ c } => {
                $visit(a);
                $visit(b);
                $visit(c);
            }
            Expression::Literal(_)
            | Expression::Constant(_)
            | Expression::Override(_)
            | Expression::ZeroValue(_)
            | Expression::FunctionArgument(_)
            | Expression::GlobalVariable(_)
            | Expression::LocalVariable(_)
            | Expression::CallResult(_)
            | Expression::AtomicResult { .. }
            | Expression::WorkGroupUniformLoadResult { .. }
            | Expression::RayQueryProceedResult
            | Expression::SubgroupBallotResult
            | Expression::SubgroupOperationResult { .. }
            | Expression::CooperativeLoad { .. } => {}
        }
    };
}

/// What keeps a stage in its folded form.
#[derive(Debug)]
pub(crate) struct Unsupported(pub(crate) &'static str);

/// Flattens every entry point of `module`. On an error the module is left
/// part flattened: the caller builds the stage again without flattening.
pub(crate) fn flatten(module: &mut Module) -> Result<(), Unsupported> {
    rebuild_entry_points(module, |module, function| {
        let mut current = inline_and_split(module, function)?;
        // Each rebuild folds what the last one made constant, and a branch
        // gone may leave a local one store that forwards.
        for _ in 0..FORWARD_ROUNDS {
            let forwarded = Builder::rebuild(module, &current, &forwards(&current))?;
            let shrank = forwarded.expressions.len() < current.expressions.len();
            current = forwarded;
            if !shrank {
                break;
            }
        }
        Builder::rebuild(module, &current, &liveness(&current))
    })
}

fn rebuild_entry_points(
    module: &mut Module,
    rebuild: impl Fn(&Module, &Function) -> Result<Function, Unsupported>,
) -> Result<(), Unsupported> {
    let mut entry_points = std::mem::take(&mut module.entry_points);
    let rebuilt = entry_points
        .iter()
        .map(|entry_point| rebuild(module, &entry_point.function))
        .collect::<Result<Vec<_>, _>>();
    let result = rebuilt.map(|functions| {
        for (entry_point, function) in entry_points.iter_mut().zip(functions) {
            entry_point.function = function;
        }
    });
    module.entry_points = entry_points;
    result
}

/// Inlines every call, makes private variables locals and splits struct
/// locals into their members, then forwards each local's one store.
fn inline_and_split(module: &Module, function: &Function) -> Result<Function, Unsupported> {
    let inline = Rules {
        inline: inlined(module, function),
        ..Rules::default()
    };
    let mut current = Builder::rebuild(module, function, &inline)?;
    let privates = Rules {
        private_locals: privates_to_localize(module, &current),
        ..Rules::default()
    };
    if privates.private_locals.contains(&true) {
        current = Builder::rebuild(module, &current, &privates)?;
    }
    // A member that is itself a struct splits on the next round.
    for _ in 0..4 {
        let split = splits(module, &current);
        if split.iter().all(Option::is_none) {
            break;
        }
        let rules = Rules {
            split,
            ..Rules::default()
        };
        current = Builder::rebuild(module, &current, &rules)?;
    }
    Builder::rebuild(module, &current, &forwards(&current))
}

/// Expressions inlining may add by copying a callee's body more than once.
const INLINE_GROWTH: usize = 96;

/// The most expressions a callee copied at more than one call site may
/// evaluate. Larger ones stay functions: copied, the trapezoid frame of the
/// shape shader's vertex stage cost Mali-G76 more arithmetic on its longest
/// path (malioc 8.03 to 8.47 cycles).
const SMALL_CALLEE: usize = 16;

/// By function: whether calls to it from `entry` are inlined. A callee
/// whose body would appear once is; one copied more often is when it is
/// small ([`SMALL_CALLEE`]) and the copies add at most [`INLINE_GROWTH`]
/// expressions. A callee whose `return` sits in a loop or a switch never
/// is.
fn inlined(module: &Module, entry: &Function) -> Vec<bool> {
    let count = module.functions.len();
    let mut calls_from: Vec<Vec<Handle<Function>>> = Vec::with_capacity(count);
    for (_, function) in module.functions.iter() {
        calls_from.push(called(&function.body));
    }
    // Callers come before their callees: naga keeps a function after the
    // ones it calls, so the reverse order of the arena.
    let mut copies = vec![0usize; count];
    for callee in called(&entry.body) {
        copies[callee.index()] += 1;
    }
    let mut inline = vec![false; count];
    for (handle, function) in module.functions.iter().rev() {
        let index = handle.index();
        let copied = copies[index];
        inline[index] = copied > 0
            && returns_outside_loops(&function.body)
            && (copied == 1 || emitted(&function.body) <= SMALL_CALLEE)
            && emitted(&function.body) * (copied - 1) <= INLINE_GROWTH;
        // Each copy of an inlined body calls its callees again; a function
        // that stays calls them from its one body.
        let instances = if inline[index] {
            copied
        } else {
            usize::from(copied > 0)
        };
        for &callee in &calls_from[index] {
            copies[callee.index()] += instances;
        }
    }
    inline
}

/// The expressions `block` evaluates: the work a copy of it adds.
fn emitted(block: &Block) -> usize {
    let mut count = 0;
    visit_statements(block, &mut |statement| {
        if let Statement::Emit(ref range) = *statement {
            count += range.clone().count();
        }
    });
    count
}

/// The function each call in `block` calls, once per call.
fn called(block: &Block) -> Vec<Handle<Function>> {
    let mut callees = Vec::new();
    visit_statements(block, &mut |statement| {
        if let Statement::Call { function, .. } = *statement {
            callees.push(function);
        }
    });
    callees
}

/// Forwarding rounds at most: the shape shader's stages stop shrinking
/// after three.
const FORWARD_ROUNDS: usize = 6;

/// The local a pointer names, when it reaches the local through members
/// alone.
fn member_root(
    function: &Function,
    mut pointer: Handle<Expression>,
) -> Option<Handle<LocalVariable>> {
    loop {
        match function.expressions[pointer] {
            Expression::LocalVariable(local) => return Some(local),
            Expression::AccessIndex { base, .. } => pointer = base,
            _ => return None,
        }
    }
}

/// What a rebuild changes; the default copies the function as it is.
#[derive(Default)]
struct Rules {
    /// By function: whether a call to it is inlined.
    inline: Vec<bool>,
    /// By global: whether it is a private variable the function keeps as a
    /// local of its own.
    private_locals: Vec<bool>,
    /// By local: the member types a struct local splits into.
    split: Vec<Option<Vec<Handle<Type>>>>,
    /// By local: whether a load reads it; stores to the others are dropped.
    read: Option<Vec<bool>>,
    /// By expression: whether a kept statement needs it.
    live: Option<Vec<bool>>,
    /// By local: the value every load of it reads.
    forward: Vec<Option<Forward>>,
}

/// The value each load of a local reads.
#[derive(Clone, Copy)]
enum Forward {
    /// Its one store's value, which runs before every load.
    Stored(Handle<Expression>),
    /// Its initializer: nothing stores to it.
    Init(Handle<Expression>),
    /// Zero: it has no initializer and nothing stores to it.
    Zero,
}

/// For each local of `function`, the value its loads read where that is
/// one value: a local nothing stores to, or one stored once at the top of
/// the function before every load of it. Any other use of its pointer, a
/// store to a part of it or a store in a branch keeps it.
fn forwards(function: &Function) -> Rules {
    let uses = LocalUses::of(function);
    Rules {
        forward: function
            .local_variables
            .iter()
            .map(|(local, variable)| uses.forward(local, variable))
            .collect(),
        ..Rules::default()
    }
}

/// How a function's statements use each of its locals.
struct LocalUses {
    /// By local: the stores to it as a whole.
    stores: Vec<usize>,
    /// By local: its last store's value and position, when at the top.
    stored: Vec<Option<(Handle<Expression>, usize)>>,
    /// By local: the position of its first load.
    first_load: Vec<Option<usize>>,
    /// By local: whether something else than a load or a whole store uses
    /// its pointer.
    kept: Vec<bool>,
}

impl LocalUses {
    fn of(function: &Function) -> Self {
        let count = function.local_variables.len();
        let mut uses = Self {
            stores: vec![0; count],
            stored: vec![None; count],
            first_load: vec![None; count],
            kept: vec![false; count],
        };
        for (_, expression) in function.expressions.iter() {
            uses.note_expression(function, expression);
        }
        let mut position = 0;
        walk_positions(
            &function.body,
            None,
            &mut position,
            &mut |statement, top, at| {
                uses.note_statement(function, statement, top, at);
            },
        );
        uses
    }

    fn keep(&mut self, function: &Function, pointer: Handle<Expression>) {
        if let Some(local) = root_local(function, pointer) {
            self.kept[local.index()] = true;
        }
    }

    fn note_expression(&mut self, function: &Function, expression: &Expression) {
        let through_members = |pointer| member_root(function, pointer).is_some();
        match *expression {
            Expression::Load { pointer } if through_members(pointer) => {}
            Expression::AccessIndex { base, .. } if through_members(base) => {}
            ref other => for_each_operand(other, &mut |operand| self.keep(function, operand)),
        }
    }

    fn note_statement(&mut self, function: &Function, statement: &Statement, top: bool, at: usize) {
        match *statement {
            Statement::Emit(ref range) => {
                for handle in range.clone() {
                    if let Expression::Load { pointer } = function.expressions[handle]
                        && let Some(local) = member_root(function, pointer)
                    {
                        let first = &mut self.first_load[local.index()];
                        *first = Some(first.map_or(at, |first| first.min(at)));
                    }
                }
            }
            Statement::Store { pointer, value } => match function.expressions[pointer] {
                Expression::LocalVariable(local) => {
                    self.stores[local.index()] += 1;
                    self.stored[local.index()] = top.then_some((value, at));
                }
                _ => self.keep(function, pointer),
            },
            ref other => {
                for_each_statement_operand(other, &mut |operand| self.keep(function, operand));
            }
        }
    }

    /// The value every load of `local` reads, if one value.
    fn forward(&self, local: Handle<LocalVariable>, variable: &LocalVariable) -> Option<Forward> {
        let index = local.index();
        if self.kept[index] {
            return None;
        }
        match (self.stores[index], variable.init, self.stored[index]) {
            (0, Some(init), _) => Some(Forward::Init(init)),
            (0, None, _) => Some(Forward::Zero),
            (1, None, Some((value, at))) if self.first_load[index].is_none_or(|load| load > at) => {
                Some(Forward::Stored(value))
            }
            _ => None,
        }
    }
}

/// Visits every statement with whether it sits at the top of the function,
/// where blocks do not count, and the position of the top statement it is
/// in, counted in the order the top statements run.
fn walk_positions(
    block: &Block,
    top: Option<usize>,
    position: &mut usize,
    visit: &mut impl FnMut(&Statement, bool, usize),
) {
    for statement in block {
        let at = top.unwrap_or_else(|| {
            *position += 1;
            *position
        });
        visit(statement, top.is_none(), at);
        match *statement {
            Statement::Block(ref inner) => walk_positions(inner, top, position, visit),
            Statement::If {
                ref accept,
                ref reject,
                ..
            } => {
                walk_positions(accept, Some(at), position, visit);
                walk_positions(reject, Some(at), position, visit);
            }
            Statement::Switch { ref cases, .. } => cases
                .iter()
                .for_each(|case| walk_positions(&case.body, Some(at), position, visit)),
            Statement::Loop {
                ref body,
                ref continuing,
                ..
            } => {
                walk_positions(body, Some(at), position, visit);
                walk_positions(continuing, Some(at), position, visit);
            }
            _ => {}
        }
    }
}

/// The struct locals of `function` that split: those whose pointer is only
/// loaded, stored or narrowed to a member.
fn splits(module: &Module, function: &Function) -> Vec<Option<Vec<Handle<Type>>>> {
    let mut split: Vec<Option<Vec<Handle<Type>>>> = function
        .local_variables
        .iter()
        .map(|(_, local)| match module.types[local.ty].inner {
            TypeInner::Struct { ref members, .. }
                if local.init.is_none_or(|init| {
                    matches!(
                        function.expressions[init],
                        Expression::ZeroValue(_) | Expression::Compose { .. }
                    )
                }) =>
            {
                Some(members.iter().map(|member| member.ty).collect())
            }
            _ => None,
        })
        .collect();
    let mut keep = |operand: Handle<Expression>| {
        if let Expression::LocalVariable(local) = function.expressions[operand] {
            split[local.index()] = None;
        }
    };
    for (_, expression) in function.expressions.iter() {
        if !matches!(
            expression,
            Expression::AccessIndex { .. } | Expression::Load { .. }
        ) {
            for_each_operand(expression, &mut keep);
        }
    }
    visit_statements(&function.body, &mut |statement| match *statement {
        Statement::Store { value, .. } => keep(value),
        ref other => for_each_statement_operand(other, &mut keep),
    });
    split
}

/// The locals some kept load reads and the expressions kept statements need,
/// found together: a store is kept only to a local something reads.
fn liveness(function: &Function) -> Rules {
    let mut read = vec![false; function.local_variables.len()];
    loop {
        let mut live = vec![false; function.expressions.len()];
        mark_effects(&function.body, function, &read, &mut live);
        let mut changed = false;
        for (handle, expression) in function.expressions.iter() {
            if live[handle.index()]
                && let Expression::LocalVariable(local) = *expression
                && !read[local.index()]
            {
                read[local.index()] = true;
                changed = true;
            }
        }
        if !changed {
            return Rules {
                read: Some(read),
                live: Some(live),
                ..Rules::default()
            };
        }
    }
}

/// The local a pointer reaches, through members and elements.
fn root_local(
    function: &Function,
    mut pointer: Handle<Expression>,
) -> Option<Handle<LocalVariable>> {
    loop {
        match function.expressions[pointer] {
            Expression::LocalVariable(local) => return Some(local),
            Expression::AccessIndex { base, .. } | Expression::Access { base, .. } => {
                pointer = base;
            }
            _ => return None,
        }
    }
}

/// Marks what the statements of `block` with an effect need, and tells
/// whether one has an effect: a store to a local nothing reads has none.
fn mark_effects(block: &Block, function: &Function, read: &[bool], live: &mut [bool]) -> bool {
    let mut effect = false;
    for statement in block {
        effect |= match *statement {
            Statement::Emit(_) => false,
            Statement::Store { pointer, .. }
                if root_local(function, pointer).is_some_and(|local| !read[local.index()]) =>
            {
                false
            }
            Statement::Block(ref inner) => mark_effects(inner, function, read, live),
            Statement::If {
                condition,
                ref accept,
                ref reject,
            } => {
                let accepted = mark_effects(accept, function, read, live);
                let rejected = mark_effects(reject, function, read, live);
                if accepted || rejected {
                    mark(condition, function, live);
                }
                accepted || rejected
            }
            Statement::Switch {
                selector,
                ref cases,
            } => {
                let mut any = false;
                for case in cases {
                    any |= mark_effects(&case.body, function, read, live);
                }
                if any {
                    mark(selector, function, live);
                }
                any
            }
            Statement::Loop {
                ref body,
                ref continuing,
                break_if,
            } => {
                mark_effects(body, function, read, live);
                mark_effects(continuing, function, read, live);
                if let Some(condition) = break_if {
                    mark(condition, function, live);
                }
                true
            }
            ref other => {
                for_each_statement_operand(other, &mut |operand| mark(operand, function, live));
                true
            }
        };
    }
    effect
}

fn mark(handle: Handle<Expression>, function: &Function, live: &mut [bool]) {
    if live[handle.index()] {
        return;
    }
    live[handle.index()] = true;
    for_each_operand(&function.expressions[handle], &mut |operand| {
        mark(operand, function, live);
    });
}

/// By global: whether it is a private variable `function` can keep as a
/// local of its own, because no function it still calls, directly or
/// through others, reads or writes it.
fn privates_to_localize(module: &Module, function: &Function) -> Vec<bool> {
    let mut localize: Vec<bool> = module
        .global_variables
        .iter()
        .map(|(_, global)| global.space == AddressSpace::Private)
        .collect();
    let mut reached = vec![false; module.functions.len()];
    let mut pending = called(&function.body);
    while let Some(callee) = pending.pop() {
        if std::mem::replace(&mut reached[callee.index()], true) {
            continue;
        }
        let callee = &module.functions[callee];
        for (_, expression) in callee.expressions.iter() {
            if let Expression::GlobalVariable(global) = *expression {
                localize[global.index()] = false;
            }
        }
        pending.extend(called(&callee.body));
    }
    localize
}

fn visit_statements(block: &Block, visit: &mut impl FnMut(&Statement)) {
    for statement in block {
        visit(statement);
        match *statement {
            Statement::Block(ref inner) => visit_statements(inner, visit),
            Statement::If {
                ref accept,
                ref reject,
                ..
            } => {
                visit_statements(accept, visit);
                visit_statements(reject, visit);
            }
            Statement::Switch { ref cases, .. } => cases
                .iter()
                .for_each(|case| visit_statements(&case.body, visit)),
            Statement::Loop {
                ref body,
                ref continuing,
                ..
            } => {
                visit_statements(body, visit);
                visit_statements(continuing, visit);
            }
            _ => {}
        }
    }
}

/// Whether every `return` of `body` is outside its loops and switches, so a
/// `break` out of one loop around the inlined body leaves it.
fn returns_outside_loops(body: &Block) -> bool {
    fn outside(block: &Block, nested: bool) -> bool {
        block.iter().all(|statement| match *statement {
            Statement::Return { .. } => !nested,
            Statement::Block(ref inner) => outside(inner, nested),
            Statement::If {
                ref accept,
                ref reject,
                ..
            } => outside(accept, nested) && outside(reject, nested),
            Statement::Switch { ref cases, .. } => {
                cases.iter().all(|case| outside(&case.body, true))
            }
            Statement::Loop {
                ref body,
                ref continuing,
                ..
            } => outside(body, true) && outside(continuing, true),
            _ => true,
        })
    }
    outside(body, false)
}

/// Whether `body` returns anywhere but where nothing of it follows: a
/// `return` there needs a `break` out of a loop around the inlined body.
fn returns_before_end(body: &Block) -> bool {
    fn at_end(block: &Block) -> usize {
        match block.last() {
            Some(Statement::Return { .. }) => 1,
            Some(Statement::Block(inner)) => at_end(inner),
            Some(Statement::If { accept, reject, .. }) => at_end(accept) + at_end(reject),
            _ => 0,
        }
    }
    let mut returns = 0;
    visit_statements(body, &mut |statement| {
        returns += usize::from(matches!(statement, Statement::Return { .. }));
    });
    returns > at_end(body)
}

/// Whether every path through `block` ends in a `return`.
fn always_returns(block: &Block) -> bool {
    match block.last() {
        Some(Statement::Return { .. }) => true,
        Some(Statement::Block(inner)) => always_returns(inner),
        Some(Statement::If { accept, reject, .. }) => {
            always_returns(accept) && always_returns(reject)
        }
        _ => false,
    }
}

/// `block` with the statements after an `if` one of whose sides always
/// returns moved into its other side, so a callee's `return`s end it and
/// the inlined body needs no loop to leave early.
fn without_early_returns(block: Block) -> Block {
    let mut statements: std::collections::VecDeque<(Statement, Span)> =
        block.span_into_iter().collect();
    let mut rewritten = Block::with_capacity(statements.len());
    while let Some((statement, span)) = statements.pop_front() {
        let statement = match statement {
            Statement::If {
                condition,
                accept,
                reject,
            } => {
                let mut accept = without_early_returns(accept);
                let mut reject = without_early_returns(reject);
                let (accept_returns, reject_returns) =
                    (always_returns(&accept), always_returns(&reject));
                if accept_returns != reject_returns {
                    let side = if accept_returns {
                        &mut reject
                    } else {
                        &mut accept
                    };
                    for (statement, span) in statements.drain(..) {
                        side.push(statement, span);
                    }
                    *side = without_early_returns(std::mem::take(side));
                } else if accept_returns {
                    statements.clear();
                }
                Statement::If {
                    condition,
                    accept,
                    reject,
                }
            }
            Statement::Block(inner) => Statement::Block(without_early_returns(inner)),
            other => other,
        };
        rewritten.push(statement, span);
    }
    rewritten
}

/// Whether nothing after `statement` in its block runs. A `discard` is not
/// such a statement here: a callee's body may end in one, and the `return`
/// of the function it was inlined into must stay, or Metal refuses a
/// fragment entry point that ends without one.
fn terminates(statement: &Statement) -> bool {
    match statement {
        Statement::Return { .. } | Statement::Break | Statement::Continue => true,
        Statement::Block(block) => block.last().is_some_and(terminates),
        _ => false,
    }
}

/// Whether a statement, not an `Emit`, makes the expression.
fn is_statement_result(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::CallResult(_)
            | Expression::AtomicResult { .. }
            | Expression::WorkGroupUniformLoadResult { .. }
            | Expression::RayQueryProceedResult
            | Expression::SubgroupBallotResult
            | Expression::SubgroupOperationResult { .. }
    )
}

/// The boolean the constants give the source expression `handle`, if any.
fn decided_bool(frame: &Frame, handle: Handle<Expression>) -> Option<bool> {
    match frame.decided[handle.index()] {
        Some(crate::shader_fold::Value::Bool(value)) => Some(value),
        _ => None,
    }
}

/// The side of `left op right` that decides the result where the other
/// is the operator's neutral constant: `x` of `true & x` or `false | x`.
fn undecided_side(
    frame: &Frame,
    op: naga::BinaryOperator,
    left: Handle<Expression>,
    right: Handle<Expression>,
) -> Option<Handle<Expression>> {
    let neutral = match op {
        naga::BinaryOperator::LogicalAnd | naga::BinaryOperator::And => true,
        naga::BinaryOperator::LogicalOr | naga::BinaryOperator::InclusiveOr => false,
        _ => return None,
    };
    if decided_bool(frame, left) == Some(neutral) {
        Some(right)
    } else if decided_bool(frame, right) == Some(neutral) {
        Some(left)
    } else {
        None
    }
}

/// One source function being copied: the entry point, or a callee inlined
/// into it.
struct Frame<'f> {
    source: &'f Function,
    /// By source expression: the value its constants give it.
    decided: Vec<Option<crate::shader_fold::Value>>,
    /// By source expression: its copy.
    copies: Vec<Option<Handle<Expression>>>,
    /// By source local: its copy, made on first use.
    locals: Vec<Option<Handle<LocalVariable>>>,
    /// An inlined callee's argument values, the pointer its `return` stores
    /// through, and whether a `return` leaves the loop around its body.
    inlined: Option<Inlined>,
}

struct Inlined {
    arguments: Vec<Handle<Expression>>,
    result: Option<Handle<Expression>>,
    leaves_loop: bool,
}

impl<'f> Frame<'f> {
    fn new(
        source: &'f Function,
        decided: Vec<Option<crate::shader_fold::Value>>,
        inlined: Option<Inlined>,
    ) -> Self {
        Self {
            source,
            decided,
            copies: vec![None; source.expressions.len()],
            locals: vec![None; source.local_variables.len()],
            inlined,
        }
    }
}

/// A copy of one function, built statement by statement in the order the
/// statements run, so each expression comes after the ones it reads.
struct Builder<'m> {
    module: &'m Module,
    rules: &'m Rules,
    out: Function,
    /// By global: its local, made on first use.
    privates: Vec<Option<Handle<LocalVariable>>>,
    /// By source local: the member locals a split local became.
    members: Vec<Option<Vec<Handle<LocalVariable>>>>,
    /// Loops around the statement being copied.
    loop_depth: usize,
    /// By function: its body without early returns, made at its first call
    /// and taken out while it is being inlined.
    callee_bodies: Vec<Option<Block>>,
    /// By function: what its constants decide in it, kept as its body is.
    callee_decided: Vec<Option<Vec<Option<crate::shader_fold::Value>>>>,
}

impl<'m> Builder<'m> {
    fn rebuild(
        module: &'m Module,
        source: &Function,
        rules: &'m Rules,
    ) -> Result<Function, Unsupported> {
        let mut builder = Builder {
            module,
            rules,
            out: Function {
                name: source.name.clone(),
                arguments: source.arguments.clone(),
                result: source.result.clone(),
                local_variables: Arena::new(),
                expressions: Arena::new(),
                named_expressions: Default::default(),
                body: Block::new(),
                diagnostic_filter_leaf: source.diagnostic_filter_leaf,
            },
            privates: vec![None; module.global_variables.len()],
            members: vec![None; source.local_variables.len()],
            loop_depth: 0,
            callee_bodies: vec![None; module.functions.len()],
            callee_decided: vec![None; module.functions.len()],
        };
        let mut frame = Frame::new(source, crate::shader_fold::decided(module, source), None);
        builder.out.body = builder.block(&mut frame, &source.body)?;
        Ok(builder.out)
    }

    fn append(&mut self, expression: Expression) -> Handle<Expression> {
        self.out.expressions.append(expression, Span::UNDEFINED)
    }

    /// Emits the expressions appended since `start` that an `Emit` makes.
    fn emit_since(&self, start: usize, block: &mut Block) {
        let mut run: Option<(Handle<Expression>, Handle<Expression>)> = None;
        for handle in self.out.expressions.range_from(start) {
            let expression = &self.out.expressions[handle];
            if !expression.needs_pre_emit() && !is_statement_result(expression) {
                run = Some(run.map_or((handle, handle), |(first, _)| (first, handle)));
            } else if let Some((first, last)) = run.take() {
                block.push(
                    Statement::Emit(Range::new_from_bounds(first, last)),
                    Span::UNDEFINED,
                );
            }
        }
        if let Some((first, last)) = run {
            block.push(
                Statement::Emit(Range::new_from_bounds(first, last)),
                Span::UNDEFINED,
            );
        }
    }

    fn local(
        &mut self,
        frame: &mut Frame,
        local: Handle<LocalVariable>,
    ) -> Result<Handle<LocalVariable>, Unsupported> {
        if let Some(copy) = frame.locals[local.index()] {
            return Ok(copy);
        }
        let variable = &frame.source.local_variables[local];
        let init = variable
            .init
            .map(|init| self.expression(frame, init))
            .transpose()?;
        let copy = self.out.local_variables.append(
            LocalVariable {
                name: variable.name.clone(),
                ty: variable.ty,
                init,
            },
            Span::UNDEFINED,
        );
        frame.locals[local.index()] = Some(copy);
        Ok(copy)
    }

    /// Member `index` of a split local: the local it became, made with the
    /// others on first use, each starting as its member of the local's
    /// initializer.
    fn member(
        &mut self,
        frame: &mut Frame,
        local: Handle<LocalVariable>,
        index: usize,
    ) -> Result<Handle<LocalVariable>, Unsupported> {
        if self.members[local.index()].is_none() {
            let rules = self.rules;
            let types = rules.split[local.index()]
                .as_deref()
                .ok_or(Unsupported("a member of a local that keeps its struct"))?;
            let source = frame.source;
            let variable = &source.local_variables[local];
            let mut members = Vec::with_capacity(types.len());
            for (position, &ty) in types.iter().enumerate() {
                let init = match variable.init.map(|init| &source.expressions[init]) {
                    Some(Expression::Compose { components, .. }) => {
                        let component = *components
                            .get(position)
                            .ok_or(Unsupported("a struct initializer without a member"))?;
                        Some(self.expression(frame, component)?)
                    }
                    _ => None,
                };
                let name = variable
                    .name
                    .as_ref()
                    .map(|name| format!("{name}_{position}"));
                members.push(
                    self.out
                        .local_variables
                        .append(LocalVariable { name, ty, init }, Span::UNDEFINED),
                );
            }
            self.members[local.index()] = Some(members);
        }
        self.members[local.index()]
            .as_ref()
            .and_then(|members| members.get(index))
            .copied()
            .ok_or(Unsupported("a member past the struct's end"))
    }

    /// How many members a split local has.
    fn member_count(&self, local: Handle<LocalVariable>) -> usize {
        self.rules.split[local.index()].as_ref().map_or(0, Vec::len)
    }

    /// The source local `pointer` names, when it splits.
    fn split_local(
        &self,
        frame: &Frame,
        pointer: Handle<Expression>,
    ) -> Option<Handle<LocalVariable>> {
        match frame.source.expressions[pointer] {
            Expression::LocalVariable(local)
                if self
                    .rules
                    .split
                    .get(local.index())
                    .is_some_and(Option::is_some) =>
            {
                Some(local)
            }
            _ => None,
        }
    }

    /// The value a load through `pointer` reads when its local forwards:
    /// the forwarded value, narrowed to the members the pointer names.
    fn forwarded(
        &mut self,
        frame: &mut Frame,
        pointer: Handle<Expression>,
    ) -> Result<Option<Handle<Expression>>, Unsupported> {
        match frame.source.expressions[pointer] {
            Expression::LocalVariable(local) => {
                let Some(forward) = self.rules.forward.get(local.index()).copied().flatten() else {
                    return Ok(None);
                };
                let value = match forward {
                    Forward::Stored(value) | Forward::Init(value) => {
                        self.expression(frame, value)?
                    }
                    Forward::Zero => {
                        let ty = frame.source.local_variables[local].ty;
                        self.append(Expression::ZeroValue(ty))
                    }
                };
                Ok(Some(value))
            }
            Expression::AccessIndex { base, index } => Ok(self
                .forwarded(frame, base)?
                .map(|value| self.member_of(value, index))),
            _ => Ok(None),
        }
    }

    /// Member `index` of the value `value`.
    fn member_of(&mut self, value: Handle<Expression>, index: u32) -> Handle<Expression> {
        match self.composed_member(value, index) {
            Some(component) => component,
            None => self.append(Expression::AccessIndex { base: value, index }),
        }
    }

    /// A load through `pointer`: the forwarded value, the members of a split
    /// local composed again, or a load of the copied pointer.
    fn load(
        &mut self,
        frame: &mut Frame,
        pointer: Handle<Expression>,
    ) -> Result<Handle<Expression>, Unsupported> {
        if let Some(value) = self.forwarded(frame, pointer)? {
            return Ok(value);
        }
        if let Some(local) = self.split_local(frame, pointer) {
            let ty = frame.source.local_variables[local].ty;
            let count = self.member_count(local);
            let mut components = Vec::with_capacity(count);
            for index in 0..count {
                let member = self.member(frame, local, index)?;
                let pointer = self.append(Expression::LocalVariable(member));
                components.push(self.append(Expression::Load { pointer }));
            }
            return Ok(self.append(Expression::Compose { ty, components }));
        }
        let pointer = self.expression(frame, pointer)?;
        Ok(self.append(Expression::Load { pointer }))
    }

    /// Member `index` of `composed` when that is one of its components: a
    /// struct's or array's, or a vector's or matrix's composed of one
    /// component per element.
    fn composed_member(
        &self,
        composed: Handle<Expression>,
        index: u32,
    ) -> Option<Handle<Expression>> {
        let Expression::Compose { ty, ref components } = self.out.expressions[composed] else {
            return None;
        };
        let elements = match self.module.types[ty].inner {
            TypeInner::Struct { ref members, .. } => members.len(),
            TypeInner::Array {
                size: naga::ArraySize::Constant(size),
                ..
            } => size.get() as usize,
            TypeInner::Vector { size, .. } => size as usize,
            TypeInner::Matrix { columns, .. } => columns as usize,
            _ => return None,
        };
        (components.len() == elements)
            .then(|| components.get(index as usize).copied())
            .flatten()
    }

    fn private(
        &mut self,
        global: Handle<naga::GlobalVariable>,
    ) -> Result<Handle<LocalVariable>, Unsupported> {
        if let Some(local) = self.privates[global.index()] {
            return Ok(local);
        }
        let variable = &self.module.global_variables[global];
        let init = match variable
            .init
            .map(|init| &self.module.global_expressions[init])
        {
            None | Some(&Expression::ZeroValue(_)) => None,
            Some(&Expression::Literal(literal)) => Some(self.append(Expression::Literal(literal))),
            Some(&Expression::Constant(constant)) => {
                Some(self.append(Expression::Constant(constant)))
            }
            Some(_) => return Err(Unsupported("a private variable's initializer")),
        };
        let local = self.out.local_variables.append(
            LocalVariable {
                name: variable.name.clone(),
                ty: variable.ty,
                init,
            },
            Span::UNDEFINED,
        );
        self.privates[global.index()] = Some(local);
        Ok(local)
    }

    /// The copy of the source expression `handle`, made on first use after
    /// its operands.
    fn expression(
        &mut self,
        frame: &mut Frame,
        handle: Handle<Expression>,
    ) -> Result<Handle<Expression>, Unsupported> {
        if let Some(copy) = frame.copies[handle.index()] {
            return Ok(copy);
        }
        let copy = match self.decided_copy(frame, handle)? {
            Some(copy) => copy,
            None => self.copied(frame, handle)?,
        };
        frame.copies[handle.index()] = Some(copy);
        if let Some(name) = frame.source.named_expressions.get(&handle)
            && !self.out.expressions[copy].needs_pre_emit()
        {
            self.out
                .named_expressions
                .entry(copy)
                .or_insert_with(|| name.clone());
        }
        Ok(copy)
    }

    /// The copy of `handle` its constants decide: the literal it holds, the
    /// side a select takes, or the side of an `and` or `or` the other does
    /// not decide.
    fn decided_copy(
        &mut self,
        frame: &mut Frame,
        handle: Handle<Expression>,
    ) -> Result<Option<Handle<Expression>>, Unsupported> {
        let source = frame.source;
        let expression = &source.expressions[handle];
        if let Some(value) = frame.decided[handle.index()]
            && !matches!(expression, Expression::Literal(_))
        {
            return Ok(Some(self.append(Expression::Literal(value.literal()))));
        }
        let kept = match *expression {
            Expression::Select {
                condition,
                accept,
                reject,
            } => decided_bool(frame, condition).map(|taken| if taken { accept } else { reject }),
            Expression::Binary { op, left, right } => undecided_side(frame, op, left, right),
            _ => None,
        };
        kept.map(|kept| self.expression(frame, kept)).transpose()
    }

    /// The copy of `handle` made from its operands' copies.
    fn copied(
        &mut self,
        frame: &mut Frame,
        handle: Handle<Expression>,
    ) -> Result<Handle<Expression>, Unsupported> {
        let source = frame.source;
        match source.expressions[handle] {
            Expression::FunctionArgument(index) if frame.inlined.is_some() => frame
                .inlined
                .as_ref()
                .and_then(|inlined| inlined.arguments.get(index as usize))
                .copied()
                .ok_or(Unsupported("an argument the call does not pass")),
            Expression::LocalVariable(local) => {
                let copy = self.local(frame, local)?;
                Ok(self.append(Expression::LocalVariable(copy)))
            }
            Expression::GlobalVariable(global)
                if self.rules.private_locals.get(global.index()) == Some(&true) =>
            {
                let local = self.private(global)?;
                Ok(self.append(Expression::LocalVariable(local)))
            }
            Expression::AccessIndex { base, index } => self.access_index(frame, base, index),
            Expression::Load { pointer } => self.load(frame, pointer),
            Expression::CallResult(_) => Err(Unsupported("a call's result before the call")),
            ref other => {
                let mut copy = other.clone();
                try_map_operands(&mut copy, |operand| self.expression(frame, operand))?;
                Ok(self.append(copy))
            }
        }
    }

    /// Member `index` of `base`: the member's own local where `base` is a
    /// split local, a component where it was just composed.
    fn access_index(
        &mut self,
        frame: &mut Frame,
        base: Handle<Expression>,
        index: u32,
    ) -> Result<Handle<Expression>, Unsupported> {
        if let Some(local) = self.split_local(frame, base) {
            let member = self.member(frame, local, index as usize)?;
            return Ok(self.append(Expression::LocalVariable(member)));
        }
        let base = self.expression(frame, base)?;
        Ok(self.member_of(base, index))
    }

    fn block(&mut self, frame: &mut Frame, source: &Block) -> Result<Block, Unsupported> {
        let mut block = Block::with_capacity(source.len());
        for statement in source {
            self.statement(frame, statement, &mut block)?;
            if block.last().is_some_and(terminates) {
                break;
            }
        }
        Ok(block)
    }

    fn statement(
        &mut self,
        frame: &mut Frame,
        statement: &Statement,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        match *statement {
            Statement::Emit(ref range) => self.emit(frame, range, block),
            // A block's statements join the enclosing block: a value made
            // in it stays visible to what follows, as forwarding needs.
            Statement::Block(ref inner) => {
                let inner = self.block(frame, inner)?;
                block.extend_block(inner);
                Ok(())
            }
            Statement::If {
                condition,
                ref accept,
                ref reject,
            } => self.branch(frame, condition, (accept, reject), block),
            Statement::Switch {
                selector,
                ref cases,
            } => self.switch(frame, selector, cases, block),
            Statement::Loop {
                ref body,
                ref continuing,
                break_if,
            } => self.repeat(frame, (body, continuing), break_if, block),
            Statement::Break
            | Statement::Continue
            | Statement::Kill
            | Statement::ControlBarrier(_)
            | Statement::MemoryBarrier(_) => {
                block.push(statement.clone(), Span::UNDEFINED);
                Ok(())
            }
            Statement::Return { value } => self.ret(frame, value, block),
            Statement::Store { pointer, value } => self.store(frame, pointer, value, block),
            Statement::ImageStore { .. } => self.image_store(frame, statement, block),
            Statement::Call {
                function,
                ref arguments,
                result,
            } => self.call(frame, function, arguments, result, block),
            _ => Err(Unsupported("a statement the passes do not copy")),
        }
    }

    /// An `Emit`: the expressions of `range` a kept statement needs, and
    /// the operands they bring.
    fn emit(
        &mut self,
        frame: &mut Frame,
        range: &Range<Expression>,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let start = self.out.expressions.len();
        for handle in range.clone() {
            let needed = self
                .rules
                .live
                .as_ref()
                .is_none_or(|live| live[handle.index()]);
            if needed {
                self.expression(frame, handle)?;
            }
        }
        self.emit_since(start, block);
        Ok(())
    }

    /// An `if`: the side its constants decide, or both sides when either
    /// keeps a statement.
    fn branch(
        &mut self,
        frame: &mut Frame,
        condition: Handle<Expression>,
        (accept, reject): (&Block, &Block),
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        if let Some(taken) = decided_bool(frame, condition) {
            let kept = self.block(frame, if taken { accept } else { reject })?;
            block.extend_block(kept);
            return Ok(());
        }
        let accept = self.block(frame, accept)?;
        let reject = self.block(frame, reject)?;
        if !accept.is_empty() || !reject.is_empty() {
            let condition = self.expression(frame, condition)?;
            block.push(
                Statement::If {
                    condition,
                    accept,
                    reject,
                },
                Span::UNDEFINED,
            );
        }
        Ok(())
    }

    fn switch(
        &mut self,
        frame: &mut Frame,
        selector: Handle<Expression>,
        cases: &[SwitchCase],
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let mut copies = Vec::with_capacity(cases.len());
        for case in cases {
            copies.push(SwitchCase {
                value: case.value,
                body: self.block(frame, &case.body)?,
                fall_through: case.fall_through,
            });
        }
        if copies.iter().any(|case| !case.body.is_empty()) {
            let selector = self.expression(frame, selector)?;
            block.push(
                Statement::Switch {
                    selector,
                    cases: copies,
                },
                Span::UNDEFINED,
            );
        }
        Ok(())
    }

    fn repeat(
        &mut self,
        frame: &mut Frame,
        (body, continuing): (&Block, &Block),
        break_if: Option<Handle<Expression>>,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        self.loop_depth += 1;
        let copied = (|| {
            let body = self.block(frame, body)?;
            let continuing = self.block(frame, continuing)?;
            let break_if = break_if
                .map(|condition| self.expression(frame, condition))
                .transpose()?;
            Ok::<_, Unsupported>((body, continuing, break_if))
        })();
        self.loop_depth -= 1;
        let (body, continuing, break_if) = copied?;
        block.push(
            Statement::Loop {
                body,
                continuing,
                break_if,
            },
            Span::UNDEFINED,
        );
        Ok(())
    }

    fn image_store(
        &mut self,
        frame: &mut Frame,
        statement: &Statement,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let Statement::ImageStore {
            image,
            coordinate,
            array_index,
            value,
        } = *statement
        else {
            return Err(Unsupported("an image store"));
        };
        let image = self.expression(frame, image)?;
        let coordinate = self.expression(frame, coordinate)?;
        let array_index = array_index
            .map(|index| self.expression(frame, index))
            .transpose()?;
        let value = self.expression(frame, value)?;
        block.push(
            Statement::ImageStore {
                image,
                coordinate,
                array_index,
                value,
            },
            Span::UNDEFINED,
        );
        Ok(())
    }

    /// A `return`: the entry point's stays; an inlined callee's stores its
    /// value for the call's result and leaves the loop around its body.
    fn ret(
        &mut self,
        frame: &mut Frame,
        value: Option<Handle<Expression>>,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let value = value
            .map(|value| self.expression(frame, value))
            .transpose()?;
        let Some(ref inlined) = frame.inlined else {
            block.push(Statement::Return { value }, Span::UNDEFINED);
            return Ok(());
        };
        if let (Some(pointer), Some(value)) = (inlined.result, value) {
            block.push(Statement::Store { pointer, value }, Span::UNDEFINED);
        }
        if inlined.leaves_loop {
            block.push(Statement::Break, Span::UNDEFINED);
        }
        Ok(())
    }

    fn store(
        &mut self,
        frame: &mut Frame,
        pointer: Handle<Expression>,
        value: Handle<Expression>,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let unread = self.rules.read.as_ref().is_some_and(|read| {
            root_local(frame.source, pointer).is_some_and(|local| !read[local.index()])
        });
        if unread {
            return Ok(());
        }
        let start = self.out.expressions.len();
        let value = self.expression(frame, value)?;
        let Some(local) = self.split_local(frame, pointer) else {
            let pointer = self.expression(frame, pointer)?;
            self.emit_since(start, block);
            block.push(Statement::Store { pointer, value }, Span::UNDEFINED);
            return Ok(());
        };
        let count = self.member_count(local);
        let mut stores = Vec::with_capacity(count);
        for index in 0..count {
            let member = self.member(frame, local, index)?;
            let member_index =
                u32::try_from(index).map_err(|_| Unsupported("a struct too wide"))?;
            let component = match self.out.expressions[value] {
                Expression::Compose { .. } => self
                    .composed_member(value, member_index)
                    .ok_or(Unsupported("a struct composed without its members"))?,
                Expression::ZeroValue(_) => {
                    let ty = self.out.local_variables[member].ty;
                    self.append(Expression::ZeroValue(ty))
                }
                _ => self.append(Expression::AccessIndex {
                    base: value,
                    index: member_index,
                }),
            };
            stores.push((self.append(Expression::LocalVariable(member)), component));
        }
        self.emit_since(start, block);
        for (pointer, value) in stores {
            block.push(Statement::Store { pointer, value }, Span::UNDEFINED);
        }
        Ok(())
    }

    fn call(
        &mut self,
        frame: &mut Frame,
        function: Handle<Function>,
        arguments: &[Handle<Expression>],
        result: Option<Handle<Expression>>,
        block: &mut Block,
    ) -> Result<(), Unsupported> {
        let arguments = arguments
            .iter()
            .map(|&argument| self.expression(frame, argument))
            .collect::<Result<Vec<_>, _>>()?;
        if self.rules.inline.get(function.index()) != Some(&true) {
            let result = result.map(|result| {
                let copy = self.append(Expression::CallResult(function));
                frame.copies[result.index()] = Some(copy);
                copy
            });
            block.push(
                Statement::Call {
                    function,
                    arguments,
                    result,
                },
                Span::UNDEFINED,
            );
            return Ok(());
        }
        let callee = &self.module.functions[function];
        let slot = callee.result.as_ref().map(|returned| {
            let local = self.out.local_variables.append(
                LocalVariable {
                    name: callee.name.as_ref().map(|name| format!("{name}_result")),
                    ty: returned.ty,
                    init: None,
                },
                Span::UNDEFINED,
            );
            self.append(Expression::LocalVariable(local))
        });
        let callee_body = self.callee_bodies[function.index()]
            .take()
            .unwrap_or_else(|| without_early_returns(callee.body.clone()));
        let leaves_loop = returns_before_end(&callee_body);
        let decided = self.callee_decided[function.index()]
            .take()
            .unwrap_or_else(|| crate::shader_fold::decided(self.module, callee));
        let mut inner = Frame::new(
            callee,
            decided,
            Some(Inlined {
                arguments,
                result: slot,
                leaves_loop,
            }),
        );
        let mut body = Block::new();
        // A callee's locals start as declared at every call: in a loop, a
        // local made once would keep the last call's values.
        if self.loop_depth > 0 {
            for (local, variable) in callee.local_variables.iter() {
                let copy = self.local(&mut inner, local)?;
                let start = self.out.expressions.len();
                let value = match variable.init {
                    Some(init) => self.constant(&inner, init)?,
                    None => self.append(Expression::ZeroValue(variable.ty)),
                };
                let pointer = self.append(Expression::LocalVariable(copy));
                self.emit_since(start, &mut body);
                body.push(Statement::Store { pointer, value }, Span::UNDEFINED);
            }
        }
        self.loop_depth += usize::from(leaves_loop);
        let inlined = self.block(&mut inner, &callee_body);
        self.callee_bodies[function.index()] = Some(callee_body);
        self.callee_decided[function.index()] = Some(std::mem::take(&mut inner.decided));
        let inlined = inlined?;
        self.loop_depth -= usize::from(leaves_loop);
        body.extend_block(inlined);
        if leaves_loop {
            if !body.last().is_some_and(terminates) {
                body.push(Statement::Break, Span::UNDEFINED);
            }
            block.push(
                Statement::Loop {
                    body,
                    continuing: Block::new(),
                    break_if: None,
                },
                Span::UNDEFINED,
            );
        } else {
            block.extend_block(body);
        }
        if let (Some(result), Some(pointer)) = (result, slot) {
            let start = self.out.expressions.len();
            let load = self.append(Expression::Load { pointer });
            self.emit_since(start, block);
            frame.copies[result.index()] = Some(load);
        }
        Ok(())
    }

    /// A new copy of the constant expression `init` of `frame`'s source.
    fn constant(
        &mut self,
        frame: &Frame,
        init: Handle<Expression>,
    ) -> Result<Handle<Expression>, Unsupported> {
        let mut copy = frame.source.expressions[init].clone();
        if !matches!(
            copy,
            Expression::Literal(_)
                | Expression::Constant(_)
                | Expression::ZeroValue(_)
                | Expression::Compose { .. }
                | Expression::Splat { .. }
        ) {
            return Err(Unsupported("a local's initializer"));
        }
        try_map_operands(&mut copy, |operand| self.constant(frame, operand))?;
        Ok(self.append(copy))
    }
}

/// Replaces each operand of `expression` by `map` of it, or gives the
/// first error `map` gave.
fn try_map_operands(
    expression: &mut Expression,
    mut map: impl FnMut(Handle<Expression>) -> Result<Handle<Expression>, Unsupported>,
) -> Result<(), Unsupported> {
    let mut failure = None;
    map_operands(expression, &mut |operand| {
        map(operand).unwrap_or_else(|error| {
            failure.get_or_insert(error);
            operand
        })
    });
    failure.map_or(Ok(()), Err)
}

/// Calls `visit` with each operand of `expression`.
fn for_each_operand(expression: &Expression, visit: &mut impl FnMut(Handle<Expression>)) {
    let mut one = |handle: &Handle<Expression>| visit(*handle);
    operands!(expression, one, ref);
}

/// The expressions a statement reads, but not those of its nested blocks.
fn for_each_statement_operand(statement: &Statement, visit: &mut impl FnMut(Handle<Expression>)) {
    match *statement {
        Statement::If { condition, .. } => visit(condition),
        Statement::Switch { selector, .. } => visit(selector),
        Statement::Loop { break_if, .. } => break_if.into_iter().for_each(visit),
        Statement::Return { value } => value.into_iter().for_each(visit),
        Statement::Store { pointer, value } => {
            visit(pointer);
            visit(value);
        }
        Statement::ImageStore {
            image,
            coordinate,
            array_index,
            value,
        } => {
            visit(image);
            visit(coordinate);
            array_index.into_iter().for_each(&mut *visit);
            visit(value);
        }
        Statement::Call { ref arguments, .. } => arguments.iter().copied().for_each(visit),
        _ => {}
    }
}

/// Replaces each operand of `expression` by `map` of it.
fn map_operands(
    expression: &mut Expression,
    map: &mut impl FnMut(Handle<Expression>) -> Handle<Expression>,
) {
    let mut one = |handle: &mut Handle<Expression>| *handle = map(*handle);
    operands!(expression, one, ref mut);
}

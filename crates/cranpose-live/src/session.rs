use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use coroflow::MutableStateFlow;
use cranpose_coroflow::StateFlowCollect;
use serde_json::Value;

use crate::{
    Error, Expression, LiveValue, Node, Patch, Program, Registry, ValueType,
    api::{Export, Member},
    registry::Registered,
};

enum BoundExpression {
    Literal(Value),
    State(Rc<Export>),
    Text(Box<BoundExpression>),
    Action(Rc<BoundAction>),
}

struct BoundAction {
    export: Rc<Export>,
    arguments: Vec<BoundExpression>,
}

struct PreparedNode {
    id: String,
    component: Rc<Registered>,
    arguments: BTreeMap<String, BoundExpression>,
    children: Rc<[Rc<PreparedNode>]>,
}

struct Document {
    source: Program,
    root: Rc<PreparedNode>,
}

struct SessionInner {
    registry: Registry,
    document: RefCell<Document>,
    revision: MutableStateFlow<i64>,
    errors: RefCell<Vec<String>>,
}

/// One live document and its application-owned API bindings.
#[derive(Clone)]
pub struct Session(Rc<SessionInner>);

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Session {
    /// Validates and resolves a program once, before it can render.
    pub fn new(registry: Registry, program: Program) -> Result<Self, Error> {
        let root = prepare(&registry, &program)?;
        Ok(Self(Rc::new(SessionInner {
            registry,
            document: RefCell::new(Document {
                source: program,
                root,
            }),
            revision: MutableStateFlow::new(0),
            errors: RefCell::new(Vec::new()),
        })))
    }

    /// Current committed document revision.
    pub fn revision(&self) -> u64 {
        self.0.revision.value() as u64
    }

    pub(crate) fn observe_revision(&self) -> u64 {
        self.0.revision.as_state_flow().collectAsState().get() as u64
    }

    /// The catalogue available to both source translation and agent edits.
    pub fn registry(&self) -> &Registry {
        &self.0.registry
    }

    /// Takes a cheap persistent snapshot of the editable program.
    pub fn program(&self) -> Program {
        self.0.document.borrow().source.clone()
    }

    /// Validates and commits all edits together, retaining the last good document on error.
    pub fn apply(&self, patch: Patch) -> Result<u64, Error> {
        self.check_revision(patch.base_revision)?;
        if patch.edits.len() > self.0.registry.limits.edits {
            return Err(Error::Invalid(
                "patch exceeds the configured edit limit".into(),
            ));
        }
        let next = self.0.document.borrow().source.edited(patch.edits)?;
        self.replace(patch.base_revision, next)
    }

    /// Replaces the program after a source translation or full document update.
    pub fn replace(&self, base_revision: u64, program: Program) -> Result<u64, Error> {
        self.check_revision(base_revision)?;
        if self.0.document.borrow().source == program {
            return Ok(base_revision);
        }
        let root = prepare(&self.0.registry, &program)?;
        let next = self
            .0
            .revision
            .value()
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("revision exhausted".into()))?;
        *self.0.document.borrow_mut() = Document {
            source: program,
            root,
        };
        self.0.revision.set(next);
        Ok(next as u64)
    }

    /// Translates and commits a supported Rust function without invoking rustc.
    pub fn replace_source(
        &self,
        base_revision: u64,
        source: &str,
        function: &str,
    ) -> Result<u64, Error> {
        self.check_revision(base_revision)?;
        let program = crate::parse_source(&self.0.registry, source, function)?;
        self.replace(base_revision, program)
    }

    /// Invokes a node event through the same adapter used by native input callbacks.
    pub fn dispatch(&self, node: &str, event: &str) -> Result<(), Error> {
        let root = Rc::clone(&self.0.document.borrow().root);
        let node =
            find(&root, node).ok_or_else(|| Error::Invalid(format!("unknown node '{node}'")))?;
        match node.arguments.get(event) {
            Some(BoundExpression::Action(action)) => action.invoke(),
            _ => Err(Error::Invalid(format!("unknown event '{event}'"))),
        }
    }

    /// Drains native rendering or event errors since the previous call.
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.errors.borrow_mut())
    }

    fn check_revision(&self, received: u64) -> Result<(), Error> {
        let expected = self.revision();
        if received != expected {
            return Err(Error::Stale { expected, received });
        }
        Ok(())
    }

    fn render(&self, node: Rc<PreparedNode>) {
        let Some(render) = node.component.render else {
            self.record(Error::Invalid("component has no live adapter".into()));
            return;
        };
        cranpose_core::key(node.id.as_str(), || {
            if let Err(error) = render(Invocation {
                session: self.clone(),
                node: Rc::clone(&node),
            }) {
                self.record(error);
            }
        });
    }

    fn record(&self, error: Error) {
        let limit = self.0.registry.limits.diagnostics;
        if limit == 0 {
            return;
        }
        let mut errors = self.0.errors.borrow_mut();
        let message = error.to_string();
        if errors.last() == Some(&message) {
            return;
        }
        if errors.len() == limit {
            errors.remove(0);
        }
        errors.push(message);
    }
}

/// Renders the current live program with Cranpose's normal composition runtime.
#[crate::composable]
pub fn LiveView(session: Session) {
    let _ = session.observe_revision();
    let root = Rc::clone(&session.0.document.borrow().root);
    cranpose_ui::Box(
        cranpose_ui::Modifier::empty(),
        cranpose_ui::BoxSpec::default(),
        move || {
            session.render(Rc::clone(&root));
        },
    );
}

/// Checked arguments and content passed to a registered native component.
pub struct Invocation {
    session: Session,
    node: Rc<PreparedNode>,
}

impl Invocation {
    /// Reads a native scalar argument and observes any state it depends on.
    pub fn argument<T: LiveValue>(&self, name: &str) -> Result<T, Error> {
        let expression = self
            .node
            .arguments
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("missing argument '{name}'")))?;
        cranpose_core::key(name, || match expression {
            BoundExpression::Literal(value) => Ok(T::deserialize(value)?),
            expression => Ok(T::deserialize(&expression.evaluate(true)?)?),
        })
    }

    /// Obtains an event callback whose captures remain valid while the widget owns it.
    pub fn action(&self, name: &str) -> Result<Action, Error> {
        match self.node.arguments.get(name) {
            Some(BoundExpression::Action(action)) => Ok(Action {
                session: self.session.clone(),
                action: Rc::clone(action),
            }),
            _ => Err(Error::Invalid(format!("missing action '{name}'"))),
        }
    }

    /// Obtains the child program for a content closure.
    pub fn content(&self) -> Content {
        Content {
            session: self.session.clone(),
            nodes: Rc::clone(&self.node.children),
        }
    }
}

/// An event bound to one registered view-model action.
#[derive(Clone)]
pub struct Action {
    session: Session,
    action: Rc<BoundAction>,
}

impl Action {
    /// Executes the event, recording any diagnostic on the owning session.
    pub fn invoke(&self) {
        if let Err(error) = self.action.invoke() {
            self.session.record(error);
        }
    }
}

/// A retained child-program slot for a native content closure.
#[derive(Clone)]
pub struct Content {
    session: Session,
    nodes: Rc<[Rc<PreparedNode>]>,
}

impl Content {
    /// Renders children using their stable document identities.
    pub fn render(&self) {
        for node in self.nodes.iter() {
            self.session.render(Rc::clone(node));
        }
    }
}

impl BoundExpression {
    fn evaluate(&self, observe: bool) -> Result<Value, Error> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::State(export) => match &export.member {
                Member::State(read) => read(observe),
                Member::Action(_) => Err(Error::Invalid("expected state".into())),
            },
            Self::Text(value) => {
                let value = value.evaluate(observe)?;
                match value {
                    Value::String(_) => Ok(value),
                    value => Ok(Value::String(value.to_string())),
                }
            }
            Self::Action(_) => Err(Error::Invalid("an action is not a scalar".into())),
        }
    }
}

impl BoundAction {
    fn invoke(&self) -> Result<(), Error> {
        let Member::Action(call) = &self.export.member else {
            return Err(Error::Invalid("expected action".into()));
        };
        let values = self
            .arguments
            .iter()
            .map(|value| value.evaluate(false))
            .collect::<Result<Vec<_>, _>>()?;
        call(&values)
    }
}

fn find<'a>(node: &'a Rc<PreparedNode>, id: &str) -> Option<&'a PreparedNode> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, id))
}

fn prepare(registry: &Registry, program: &Program) -> Result<Rc<PreparedNode>, Error> {
    prepare_node(registry, &program.root, &mut BTreeSet::new(), 0)
}

fn prepare_node<'a>(
    registry: &Registry,
    node: &'a Node,
    ids: &mut BTreeSet<&'a str>,
    depth: usize,
) -> Result<Rc<PreparedNode>, Error> {
    if depth > registry.limits.depth || ids.len() >= registry.limits.nodes {
        return Err(Error::Invalid(
            "program exceeds the configured depth or node limit".into(),
        ));
    }
    if node.id.is_empty() || !ids.insert(node.id.as_str()) {
        return Err(Error::Invalid(format!(
            "empty or duplicate node identity '{}'",
            node.id
        )));
    }
    let component = registry.component(&node.component)?;
    if let Some(reason) = &component.schema.unavailable {
        return Err(Error::Invalid(format!("'{}': {reason}", node.component)));
    }
    if node.arguments.len() != component.schema.parameters.len() {
        return Err(Error::Invalid(format!(
            "'{}' arguments do not match its schema",
            node.id
        )));
    }
    if !node.children.is_empty() && component.schema.content.is_none() {
        return Err(Error::Invalid(format!(
            "'{}' has no content slot",
            node.component
        )));
    }
    let mut arguments = BTreeMap::new();
    for parameter in &component.schema.parameters {
        let value = node
            .arguments
            .get(&parameter.name)
            .ok_or_else(|| Error::Invalid(format!("'{}' needs '{}'", node.id, parameter.name)))?;
        arguments.insert(
            parameter.name.clone(),
            bind_expression(registry, value, parameter.value_type, 0)?,
        );
    }
    let children = node
        .children
        .iter()
        .map(|child| prepare_node(registry, child, ids, depth + 1))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Rc::new(PreparedNode {
        id: node.id.clone(),
        component: Rc::clone(component),
        arguments,
        children: children.into(),
    }))
}

fn bind_expression(
    registry: &Registry,
    expression: &Expression,
    expected: ValueType,
    depth: usize,
) -> Result<BoundExpression, Error> {
    if depth > registry.limits.depth {
        return Err(Error::Invalid(
            "expression exceeds the configured depth limit".into(),
        ));
    }
    match expression {
        Expression::Literal { value } if expected.accepts(value) => {
            Ok(BoundExpression::Literal(value.clone()))
        }
        Expression::State { api, member } => {
            let export = registry.member(api, member)?;
            if export.schema.state != Some(expected) {
                return Err(Error::Invalid(format!(
                    "'{api}.{member}' does not produce {expected:?}"
                )));
            }
            Ok(BoundExpression::State(Rc::clone(export)))
        }
        Expression::Text { value } if expected == ValueType::Text => {
            let actual = expression_type(registry, value)?;
            if matches!(actual, ValueType::Action | ValueType::Content) {
                return Err(Error::Invalid("cannot display an action or content".into()));
            }
            Ok(BoundExpression::Text(Box::new(bind_expression(
                registry,
                value,
                actual,
                depth + 1,
            )?)))
        }
        Expression::Action {
            api,
            member,
            arguments,
        } if expected == ValueType::Action => {
            let export = registry.member(api, member)?;
            if export.schema.state.is_some() || arguments.len() != export.schema.parameters.len() {
                return Err(Error::Invalid(format!(
                    "'{api}.{member}' action arguments do not match"
                )));
            }
            let arguments = arguments
                .iter()
                .zip(&export.schema.parameters)
                .map(|(value, parameter)| {
                    bind_expression(registry, value, parameter.value_type, depth + 1)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BoundExpression::Action(Rc::new(BoundAction {
                export: Rc::clone(export),
                arguments,
            })))
        }
        _ => Err(Error::Invalid(format!(
            "expression does not produce {expected:?}"
        ))),
    }
}

fn expression_type(registry: &Registry, expression: &Expression) -> Result<ValueType, Error> {
    match expression {
        Expression::Literal { value } => [
            ValueType::Text,
            ValueType::Boolean,
            ValueType::Integer,
            ValueType::Number,
        ]
        .into_iter()
        .find(|kind| kind.accepts(value))
        .ok_or_else(|| Error::Invalid("only scalar literals are supported".into())),
        Expression::State { api, member } => registry
            .member(api, member)?
            .schema
            .state
            .ok_or_else(|| Error::Invalid("expected a state member".into())),
        Expression::Text { .. } => Ok(ValueType::Text),
        Expression::Action { .. } => Ok(ValueType::Action),
    }
}

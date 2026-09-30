use std::{collections::BTreeMap, rc::Rc};

use coroflow::StateFlow;
use cranpose_coroflow::StateFlowCollect;
use serde::Serialize;
use serde_json::Value;

use crate::{Error, LiveValue, Parameter, ValueType};

/// A view model exposing state and actions to a live session.
pub trait Api: 'static {
    /// Registers this particular model instance under one session namespace.
    fn register(self: Rc<Self>, builder: &mut ApiBuilder) -> Result<(), Error>;
}

/// A discoverable view-model member.
#[derive(Clone, Debug, Serialize)]
pub struct MemberSchema {
    /// Member name within the bound API.
    pub name: String,
    /// State result type; actions use `None`.
    pub state: Option<ValueType>,
    /// Action arguments in call order.
    pub parameters: Vec<Parameter>,
}

pub(crate) type StateReader = Box<dyn Fn(bool) -> Result<Value, Error>>;
pub(crate) type ActionHandler = Box<dyn Fn(&[Value]) -> Result<(), Error>>;

pub(crate) enum Member {
    State(StateReader),
    Action(ActionHandler),
}

pub(crate) struct Export {
    pub schema: MemberSchema,
    pub member: Member,
}

/// Collects generated adapters for one view-model instance.
#[derive(Default)]
pub struct ApiBuilder {
    pub(crate) exports: BTreeMap<String, Rc<Export>>,
}

impl ApiBuilder {
    /// Exposes a state flow using Cranpose's composition-scoped collector.
    pub fn state<T: LiveValue>(&mut self, name: &str, flow: StateFlow<T>) -> Result<(), Error> {
        let read = Box::new(move |observe| {
            if observe {
                flow.collectAsState()
                    .read(|value| serde_json::to_value(value).map_err(Error::from))
            } else {
                serde_json::to_value(flow.value()).map_err(Error::from)
            }
        });
        self.insert(name, Some(T::TYPE), Vec::new(), Member::State(read))
    }

    /// Exposes an action adapter with its argument schema.
    pub fn action(
        &mut self,
        name: &str,
        parameters: Vec<Parameter>,
        action: impl Fn(&[Value]) -> Result<(), Error> + 'static,
    ) -> Result<(), Error> {
        self.insert(name, None, parameters, Member::Action(Box::new(action)))
    }

    fn insert(
        &mut self,
        name: &str,
        state: Option<ValueType>,
        parameters: Vec<Parameter>,
        member: Member,
    ) -> Result<(), Error> {
        if self.exports.contains_key(name) {
            return Err(Error::Invalid(format!("duplicate API member '{name}'")));
        }
        self.exports.insert(
            name.into(),
            Rc::new(Export {
                schema: MemberSchema {
                    name: name.into(),
                    state,
                    parameters,
                },
                member,
            }),
        );
        Ok(())
    }
}

/// Decodes one positional argument for a generated action adapter.
pub fn argument<T: LiveValue>(arguments: &[Value], index: usize) -> Result<T, Error> {
    let value = arguments
        .get(index)
        .ok_or_else(|| Error::Invalid(format!("missing action argument {index}")))?;
    Ok(T::deserialize(value)?)
}

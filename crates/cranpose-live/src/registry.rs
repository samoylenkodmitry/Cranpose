use std::{collections::BTreeMap, rc::Rc};

use serde::Serialize;

use crate::{
    Api, ApiBuilder, Error, Invocation, LiveValue, ValueType,
    api::{Export, MemberSchema},
};

/// A component or action parameter.
#[derive(Clone, Debug, Serialize)]
pub struct Parameter {
    /// Source parameter name.
    pub name: String,
    /// Required runtime representation.
    pub value_type: ValueType,
}

impl Parameter {
    /// Describes one native scalar parameter.
    pub fn value<T: LiveValue>(name: &str) -> Self {
        Self {
            name: name.into(),
            value_type: T::TYPE,
        }
    }

    /// Describes an event callback.
    pub fn action(name: &str) -> Self {
        Self {
            name: name.into(),
            value_type: ValueType::Action,
        }
    }
}

/// Runtime contract generated beside an exported composable.
#[derive(Clone, Debug, Serialize)]
pub struct ComponentSchema {
    /// Qualified Rust identity.
    pub name: String,
    /// Public documentation from the declaration.
    pub documentation: String,
    /// Original Rust signature, including signatures requiring a future adapter.
    pub signature: String,
    /// Why a declaration is discoverable but cannot yet be invoked live.
    pub unavailable: Option<String>,
    /// Named scalar and event arguments.
    pub parameters: Vec<Parameter>,
    /// Name of the optional content parameter.
    pub content: Option<String>,
}

/// A compiled component automatically registered by `#[composable]`.
pub struct Component {
    /// Constructs this component's discoverable schema.
    pub schema: fn() -> ComponentSchema,
    /// Calls the native component with checked live arguments.
    pub render: Option<fn(Invocation) -> Result<(), Error>>,
}
inventory::collect!(Component);

pub(crate) struct Registered {
    pub schema: ComponentSchema,
    pub render: Option<fn(Invocation) -> Result<(), Error>>,
}

/// The exact components and application APIs available to a session.
#[derive(Default)]
pub struct Registry {
    /// Limits applied to source translation and document updates.
    pub limits: crate::Limits,
    pub(crate) components: BTreeMap<String, Rc<Registered>>,
    pub(crate) apis: BTreeMap<String, BTreeMap<String, Rc<Export>>>,
}

/// A serializable catalogue for editors and agents.
#[derive(Serialize)]
pub struct Catalogue<'a> {
    /// Document and diagnostic limits configured by this application.
    pub limits: crate::Limits,
    /// Registered native components.
    pub components: Vec<&'a ComponentSchema>,
    /// Bound view-model namespaces and their members.
    pub apis: BTreeMap<&'a str, Vec<&'a MemberSchema>>,
}

impl Registry {
    /// Discovers linked component registrations, rejecting duplicate identities.
    pub fn discover() -> Result<Self, Error> {
        let mut registry = Self::default();
        for component in inventory::iter::<Component> {
            let schema = (component.schema)();
            match registry.components.entry(schema.name.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Rc::new(Registered {
                        schema,
                        render: component.render,
                    }));
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    return Err(Error::Invalid(format!(
                        "duplicate component '{}'",
                        entry.key()
                    )));
                }
            }
        }
        Ok(registry)
    }

    /// Binds an application-owned model without transferring its lifecycle to UI edits.
    pub fn bind<T: Api>(&mut self, name: &str, model: Rc<T>) -> Result<(), Error> {
        if self.apis.contains_key(name) {
            return Err(Error::Invalid(format!("duplicate API '{name}'")));
        }
        let mut builder = ApiBuilder::default();
        model.register(&mut builder)?;
        self.apis.insert(name.into(), builder.exports);
        Ok(())
    }

    /// Returns the schemas the running application actually supports.
    pub fn catalogue(&self) -> Catalogue<'_> {
        Catalogue {
            limits: self.limits,
            components: self.components.values().map(|item| &item.schema).collect(),
            apis: self
                .apis
                .iter()
                .map(|(name, exports)| {
                    (
                        name.as_str(),
                        exports.values().map(|item| &item.schema).collect(),
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn component(&self, name: &str) -> Result<&Rc<Registered>, Error> {
        if let Some(component) = self.components.get(name) {
            return Ok(component);
        }
        let mut matches = self
            .components
            .values()
            .filter(|item| item.schema.name.rsplit("::").next() == Some(name));
        let first = matches
            .next()
            .ok_or_else(|| Error::Invalid(format!("unknown component '{name}'")))?;
        if matches.next().is_some() {
            return Err(Error::Invalid(format!(
                "ambiguous component '{name}'; use its qualified name"
            )));
        }
        Ok(first)
    }

    pub(crate) fn member(&self, api: &str, member: &str) -> Result<&Rc<Export>, Error> {
        self.apis
            .get(api)
            .and_then(|exports| exports.get(member))
            .ok_or_else(|| Error::Invalid(format!("unknown API member '{api}.{member}'")))
    }
}

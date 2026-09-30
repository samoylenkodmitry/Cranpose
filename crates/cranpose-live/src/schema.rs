use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// The scalar types understood by the first live-program format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// UTF-8 text.
    Text,
    /// A signed 64-bit integer.
    Integer,
    /// A finite floating-point number.
    Number,
    /// A boolean.
    Boolean,
    /// A registered action with bound arguments.
    Action,
    /// A nested UI program.
    Content,
}

impl ValueType {
    pub(crate) fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Text => value.is_string(),
            Self::Integer => value.as_i64().is_some(),
            Self::Number => value.as_f64().is_some_and(f64::is_finite),
            Self::Boolean => value.is_boolean(),
            Self::Action | Self::Content => false,
        }
    }
}

/// A native value with a concrete live-program schema.
pub trait LiveValue: Serialize + DeserializeOwned + Clone + PartialEq + 'static {
    /// The runtime representation of this type.
    const TYPE: ValueType;
}

impl LiveValue for String {
    const TYPE: ValueType = ValueType::Text;
}
impl LiveValue for i64 {
    const TYPE: ValueType = ValueType::Integer;
}
impl LiveValue for f64 {
    const TYPE: ValueType = ValueType::Number;
}
impl LiveValue for bool {
    const TYPE: ValueType = ValueType::Boolean;
}

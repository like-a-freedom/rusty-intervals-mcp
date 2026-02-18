use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EventId {
    Int(i64),
    Str(String),
}

impl EventId {
    pub fn as_cow(&self) -> Cow<'_, str> {
        match self {
            EventId::Int(v) => Cow::Owned(v.to_string()),
            EventId::Str(s) => Cow::Borrowed(s),
        }
    }
}

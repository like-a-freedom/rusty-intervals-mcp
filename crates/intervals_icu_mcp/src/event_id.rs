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

impl From<&str> for EventId {
    fn from(s: &str) -> Self {
        EventId::Str(s.to_string())
    }
}

impl From<String> for EventId {
    fn from(s: String) -> Self {
        EventId::Str(s)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FolderId {
    Int(i64),
    Str(String),
}

impl FolderId {
    pub fn as_cow(&self) -> Cow<'_, str> {
        match self {
            FolderId::Int(v) => Cow::Owned(v.to_string()),
            FolderId::Str(s) => Cow::Borrowed(s),
        }
    }
}

impl From<&str> for FolderId {
    fn from(s: &str) -> Self {
        FolderId::Str(s.to_string())
    }
}

impl From<String> for FolderId {
    fn from(s: String) -> Self {
        FolderId::Str(s)
    }
}

impl From<i64> for FolderId {
    fn from(v: i64) -> Self {
        FolderId::Int(v)
    }
}

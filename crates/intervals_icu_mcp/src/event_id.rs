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

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // EventId Tests
    // ========================================================================

    #[test]
    fn test_event_id_int_variant() {
        let id = EventId::Int(123);
        assert!(matches!(id, EventId::Int(123)));
    }

    #[test]
    fn test_event_id_str_variant() {
        let id = EventId::Str("abc".into());
        assert!(matches!(id, EventId::Str(s) if s == "abc"));
    }

    #[test]
    fn test_event_id_as_cow_int() {
        let id = EventId::Int(456);
        let cow = id.as_cow();
        assert_eq!(cow, "456");
    }

    #[test]
    fn test_event_id_as_cow_str() {
        let id = EventId::Str("test_id".into());
        let cow = id.as_cow();
        assert_eq!(cow, "test_id");
    }

    #[test]
    fn test_event_id_from_str() {
        let id: EventId = "my_id".into();
        assert!(matches!(id, EventId::Str(s) if s == "my_id"));
    }

    #[test]
    fn test_event_id_from_string() {
        let id = EventId::from(String::from("string_id"));
        assert!(matches!(id, EventId::Str(s) if s == "string_id"));
    }

    #[test]
    fn test_event_id_clone() {
        let id = EventId::Int(789);
        let cloned = id.clone();
        assert_eq!(id.as_cow(), cloned.as_cow());
    }

    #[test]
    fn test_event_id_debug() {
        let id = EventId::Int(100);
        let debug = format!("{:?}", id);
        assert!(debug.contains("Int"));
    }

    // ========================================================================
    // FolderId Tests
    // ========================================================================

    #[test]
    fn test_folder_id_int_variant() {
        let id = FolderId::Int(999);
        assert!(matches!(id, FolderId::Int(999)));
    }

    #[test]
    fn test_folder_id_str_variant() {
        let id = FolderId::Str("folder_abc".into());
        assert!(matches!(id, FolderId::Str(s) if s == "folder_abc"));
    }

    #[test]
    fn test_folder_id_as_cow_int() {
        let id = FolderId::Int(111);
        let cow = id.as_cow();
        assert_eq!(cow, "111");
    }

    #[test]
    fn test_folder_id_as_cow_str() {
        let id = FolderId::Str("folder_test".into());
        let cow = id.as_cow();
        assert_eq!(cow, "folder_test");
    }

    #[test]
    fn test_folder_id_from_str() {
        let id: FolderId = "my_folder".into();
        assert!(matches!(id, FolderId::Str(s) if s == "my_folder"));
    }

    #[test]
    fn test_folder_id_from_string() {
        let id = FolderId::from(String::from("string_folder"));
        assert!(matches!(id, FolderId::Str(s) if s == "string_folder"));
    }

    #[test]
    fn test_folder_id_from_i64() {
        let id: FolderId = 222i64.into();
        assert!(matches!(id, FolderId::Int(222)));
    }

    #[test]
    fn test_folder_id_clone() {
        let id = FolderId::Str("clone_test".into());
        let cloned = id.clone();
        assert_eq!(id.as_cow(), cloned.as_cow());
    }

    #[test]
    fn test_folder_id_debug() {
        let id = FolderId::Str("debug".into());
        let debug = format!("{:?}", id);
        assert!(debug.contains("Str"));
    }

    // ========================================================================
    // Serialization Tests
    // ========================================================================

    #[test]
    fn test_event_id_serialize_int() {
        let id = EventId::Int(123);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "123");
    }

    #[test]
    fn test_event_id_serialize_str() {
        let id = EventId::Str("abc".into());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc\"");
    }

    #[test]
    fn test_event_id_deserialize_int() {
        let json = "456";
        let id: EventId = serde_json::from_str(json).unwrap();
        assert!(matches!(id, EventId::Int(456)));
    }

    #[test]
    fn test_event_id_deserialize_str() {
        let json = "\"xyz\"";
        let id: EventId = serde_json::from_str(json).unwrap();
        assert!(matches!(id, EventId::Str(s) if s == "xyz"));
    }

    #[test]
    fn test_folder_id_serialize_int() {
        let id = FolderId::Int(789);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "789");
    }

    #[test]
    fn test_folder_id_serialize_str() {
        let id = FolderId::Str("folder".into());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"folder\"");
    }

    #[test]
    fn test_folder_id_deserialize_int() {
        let json = "101";
        let id: FolderId = serde_json::from_str(json).unwrap();
        assert!(matches!(id, FolderId::Int(101)));
    }

    #[test]
    fn test_folder_id_deserialize_str() {
        let json = "\"my_folder\"";
        let id: FolderId = serde_json::from_str(json).unwrap();
        assert!(matches!(id, FolderId::Str(s) if s == "my_folder"));
    }
}

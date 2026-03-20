/// Intent layer types and contracts
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local};
use intervals_icu_client::IntervalsClient;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::RwLock;

/// Intent execution error
#[derive(Debug, thiserror::Error)]
pub enum IntentError {
    #[error("Unknown intent: {0}")]
    UnknownIntent(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Idempotency conflict: {0}")]
    IdempotencyConflict(String),
    #[error("API client error: {0}")]
    ApiClientError(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl IntentError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::ValidationError(msg.into())
    }
    pub fn api(msg: impl Into<String>) -> Self {
        Self::ApiClientError(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::InternalError(msg.into())
    }
}

/// Content block for rich text output
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Markdown {
        markdown: String,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self::Markdown {
            markdown: markdown.into(),
        }
    }
    pub fn table(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self::Table { headers, rows }
    }
}

/// Metadata for output pagination and aggregation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_created: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_modified: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_deleted: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Output from intent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentOutput {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
    #[serde(skip_serializing_if = "OutputMetadata::is_empty")]
    pub metadata: OutputMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl IntentOutput {
    pub fn new(content: Vec<ContentBlock>) -> Self {
        Self {
            content,
            suggestions: Vec::new(),
            next_actions: Vec::new(),
            metadata: OutputMetadata::default(),
            note: None,
        }
    }
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }
    pub fn with_next_actions(mut self, next_actions: Vec<String>) -> Self {
        self.next_actions = next_actions;
        self
    }
    pub fn with_metadata(mut self, metadata: OutputMetadata) -> Self {
        self.metadata = metadata;
        self
    }
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self::new(vec![ContentBlock::markdown(markdown)])
    }
}

impl OutputMetadata {
    pub fn is_empty(&self) -> bool {
        self.has_more.is_none()
            && self.next_offset.is_none()
            && self.total_count.is_none()
            && self.events_created.is_none()
            && self.events_modified.is_none()
            && self.events_deleted.is_none()
            && self.extra.is_empty()
    }
}

/// Idempotency cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyEntry {
    pub result: IntentOutput,
    pub created_at: DateTime<Local>,
    pub expires_at: DateTime<Local>,
}

impl IdempotencyEntry {
    pub fn new(result: IntentOutput, ttl: Duration) -> Self {
        let now = Local::now();
        let expires_at =
            now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::days(1));
        Self {
            result,
            created_at: now,
            expires_at,
        }
    }
    pub fn is_expired(&self) -> bool {
        Local::now() > self.expires_at
    }
}

/// In-memory idempotency cache with TTL
#[derive(Debug, Clone)]
pub struct IdempotencyCache {
    cache: Arc<RwLock<HashMap<String, IdempotencyEntry>>>,
    default_ttl: Duration,
}

impl IdempotencyCache {
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
        }
    }
    pub async fn get(&self, token: &str) -> Option<IntentOutput> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get(token) {
            if entry.is_expired() {
                cache.remove(token);
                None
            } else {
                Some(entry.result.clone())
            }
        } else {
            None
        }
    }
    pub async fn set(&self, token: &str, result: &IntentOutput) {
        self.set_with_ttl(token, result, self.default_ttl).await;
    }
    pub async fn set_with_ttl(&self, token: &str, result: &IntentOutput, ttl: Duration) {
        let mut cache = self.cache.write().await;
        let entry = IdempotencyEntry::new(result.clone(), ttl);
        cache.insert(token.to_string(), entry);
    }
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(86400))
    }
}

/// Intent handler trait
#[async_trait::async_trait]
pub trait IntentHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        idempotency_cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError>;
    fn requires_idempotency_token(&self) -> bool {
        false
    }
    fn extract_idempotency_token(&self, input: &Value) -> Option<String> {
        input
            .get("idempotency_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }
}

/// Tool definition for MCP registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            output_schema: None,
        }
    }

    pub fn with_output_schema(mut self, output_schema: Value) -> Self {
        self.output_schema = Some(output_schema);
        self
    }
}

/// Generate standard output schema for all intents
/// Matches the IntentOutput structure for consistent validation
pub fn standard_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "content": {
                "type": "array",
                "description": "Main content blocks (Markdown, tables, text)",
                "items": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["text", "markdown", "table"],
                            "description": "Content block type"
                        },
                        "text": {
                            "type": "string",
                            "description": "Plain text content"
                        },
                        "markdown": {
                            "type": "string",
                            "description": "Markdown formatted content"
                        },
                        "headers": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Table headers"
                        },
                        "rows": {
                            "type": "array",
                            "items": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "description": "Table rows"
                        }
                    },
                    "required": ["type"]
                }
            },
            "suggestions": {
                "type": "array",
                "description": "Analytical insights and recommendations",
                "items": {"type": "string"}
            },
            "next_actions": {
                "type": "array",
                "description": "Suggested next actions (with intent names)",
                "items": {"type": "string"}
            },
            "metadata": {
                "type": "object",
                "description": "Pagination and aggregation metadata",
                "properties": {
                    "has_more": {
                        "type": "boolean",
                        "description": "Whether more results are available"
                    },
                    "next_offset": {
                        "type": "string",
                        "description": "Offset for next page"
                    },
                    "total_count": {
                        "type": "integer",
                        "description": "Total count of items"
                    },
                    "events_created": {
                        "type": "integer",
                        "description": "Number of events created"
                    },
                    "events_modified": {
                        "type": "integer",
                        "description": "Number of events modified"
                    },
                    "events_deleted": {
                        "type": "integer",
                        "description": "Number of events deleted"
                    }
                }
            },
            "note": {
                "type": "string",
                "description": "Optional note (e.g., 'Cached result')"
            }
        },
        "required": ["content"]
    })
}

/// Convert IntentOutput to MCP CallToolResult
pub fn intent_output_to_call_tool_result(
    output: &IntentOutput,
) -> Result<rmcp::model::CallToolResult, serde_json::Error> {
    use rmcp::model::CallToolResult;

    let mut result = CallToolResult::success(Vec::new());
    result.structured_content = Some(serde_json::to_value(output)?);
    Ok(result)
}

/// Convert IntentError to MCP ErrorData
pub fn intent_error_to_error_data(error: &IntentError) -> rmcp::ErrorData {
    let message = match error {
        IntentError::UnknownIntent(name) => format!(
            "Unknown intent '{}'. Available: plan_training, analyze_training, modify_training, compare_periods, assess_recovery, manage_profile, manage_gear, analyze_race",
            name
        ),
        IntentError::ValidationError(msg) => format!("Invalid input: {}. Check parameters.", msg),
        IntentError::IdempotencyConflict(msg) => {
            format!("Idempotency conflict: {}. Returning cached.", msg)
        }
        IntentError::ApiClientError(msg) => format!("API error: {}. Check connection.", msg),
        IntentError::InternalError(msg) => format!("Internal error: {}. Try again.", msg),
    };
    rmcp::ErrorData::invalid_params(message, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ContentBlock Tests
    // ========================================================================

    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::text("Hello, World!");
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "Hello, World!"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_content_block_markdown() {
        let block = ContentBlock::markdown("# Header\n\nContent");
        match block {
            ContentBlock::Markdown { markdown } => {
                assert_eq!(markdown, "# Header\n\nContent")
            }
            _ => panic!("Expected Markdown variant"),
        }
    }

    #[test]
    fn test_content_block_table() {
        let headers = vec!["Name".into(), "Value".into()];
        let rows = vec![vec!["A".into(), "1".into()], vec!["B".into(), "2".into()]];
        let block = ContentBlock::table(headers.clone(), rows.clone());

        match block {
            ContentBlock::Table {
                headers: h,
                rows: r,
            } => {
                assert_eq!(h, headers);
                assert_eq!(r, rows);
            }
            _ => panic!("Expected Table variant"),
        }
    }

    #[test]
    fn test_content_block_from_string() {
        let block = ContentBlock::text(String::from("test"));
        assert!(matches!(block, ContentBlock::Text { .. }));
    }

    #[test]
    fn test_content_block_from_str() {
        let block = ContentBlock::markdown("test");
        assert!(matches!(block, ContentBlock::Markdown { .. }));
    }

    // ========================================================================
    // OutputMetadata Tests
    // ========================================================================

    #[test]
    fn test_output_metadata_default() {
        let meta = OutputMetadata::default();
        assert!(meta.is_empty());
        assert!(meta.has_more.is_none());
        assert!(meta.next_offset.is_none());
        assert!(meta.total_count.is_none());
    }

    #[test]
    fn test_output_metadata_not_empty() {
        let meta = OutputMetadata {
            total_count: Some(10),
            ..Default::default()
        };
        assert!(!meta.is_empty());
    }

    #[test]
    fn test_output_metadata_has_more() {
        let meta = OutputMetadata {
            has_more: Some(true),
            next_offset: Some("offset_10".into()),
            ..Default::default()
        };
        assert!(!meta.is_empty());
    }

    #[test]
    fn test_output_metadata_events_created() {
        let meta = OutputMetadata {
            events_created: Some(5),
            ..Default::default()
        };
        assert!(!meta.is_empty());
        assert_eq!(meta.events_created, Some(5));
    }

    #[test]
    fn test_output_metadata_events_modified() {
        let meta = OutputMetadata {
            events_modified: Some(3),
            ..Default::default()
        };
        assert!(!meta.is_empty());
        assert_eq!(meta.events_modified, Some(3));
    }

    #[test]
    fn test_output_metadata_events_deleted() {
        let meta = OutputMetadata {
            events_deleted: Some(1),
            ..Default::default()
        };
        assert!(!meta.is_empty());
        assert_eq!(meta.events_deleted, Some(1));
    }

    #[test]
    fn test_output_metadata_extra_fields() {
        let mut meta = OutputMetadata::default();
        meta.extra.insert("custom".into(), json!("value"));
        assert!(!meta.is_empty());
    }

    #[test]
    fn test_output_metadata_clone() {
        let meta = OutputMetadata {
            total_count: Some(42),
            ..Default::default()
        };
        let cloned = meta.clone();
        assert_eq!(cloned.total_count, Some(42));
    }

    // ========================================================================
    // IntentOutput Tests
    // ========================================================================

    #[test]
    fn test_intent_output_new() {
        let content = vec![ContentBlock::text("test")];
        let output = IntentOutput::new(content.clone());
        assert_eq!(output.content.len(), 1);
        assert!(output.suggestions.is_empty());
        assert!(output.next_actions.is_empty());
        assert!(output.metadata.is_empty());
        assert!(output.note.is_none());
    }

    #[test]
    fn test_intent_output_markdown() {
        let output = IntentOutput::markdown("# Test");
        assert_eq!(output.content.len(), 1);
        assert!(matches!(
            &output.content[0],
            ContentBlock::Markdown { markdown } if markdown == "# Test"
        ));
    }

    #[test]
    fn test_intent_output_with_suggestions() {
        let output = IntentOutput::markdown("Test")
            .with_suggestions(vec!["Suggestion 1".into(), "Suggestion 2".into()]);
        assert_eq!(output.suggestions.len(), 2);
        assert_eq!(output.suggestions[0], "Suggestion 1");
    }

    #[test]
    fn test_intent_output_with_next_actions() {
        let output = IntentOutput::markdown("Test")
            .with_next_actions(vec!["Action 1".into(), "Action 2".into()]);
        assert_eq!(output.next_actions.len(), 2);
        assert_eq!(output.next_actions[0], "Action 1");
    }

    #[test]
    fn test_intent_output_with_metadata() {
        let meta = OutputMetadata {
            total_count: Some(10),
            ..Default::default()
        };
        let output = IntentOutput::markdown("Test").with_metadata(meta);
        assert_eq!(output.metadata.total_count, Some(10));
    }

    #[test]
    fn test_intent_output_with_note() {
        let output = IntentOutput::markdown("Test").with_note("This is a note");
        assert_eq!(output.note, Some("This is a note".into()));
    }

    #[test]
    fn test_intent_output_chained_builders() {
        let meta = OutputMetadata {
            events_created: Some(5),
            ..Default::default()
        };

        let output = IntentOutput::markdown("# Report")
            .with_suggestions(vec!["Try this".into()])
            .with_next_actions(vec!["Do that".into()])
            .with_metadata(meta)
            .with_note("Additional info");

        assert_eq!(output.content.len(), 1);
        assert_eq!(output.suggestions.len(), 1);
        assert_eq!(output.next_actions.len(), 1);
        assert_eq!(output.metadata.events_created, Some(5));
        assert_eq!(output.note, Some("Additional info".into()));
    }

    #[test]
    fn test_intent_output_clone() {
        let output = IntentOutput::markdown("Test").with_suggestions(vec!["Suggestion".into()]);
        let cloned = output.clone();
        assert_eq!(cloned.suggestions, output.suggestions);
    }

    // ========================================================================
    // IntentError Tests
    // ========================================================================

    #[test]
    fn test_intent_error_validation() {
        let err = IntentError::validation("Missing field");
        assert!(matches!(err, IntentError::ValidationError(_)));
        assert!(err.to_string().contains("Missing field"));
    }

    #[test]
    fn test_intent_error_api() {
        let err = IntentError::api("Connection failed");
        assert!(matches!(err, IntentError::ApiClientError(_)));
        assert!(err.to_string().contains("Connection failed"));
    }

    #[test]
    fn test_intent_error_internal() {
        let err = IntentError::internal("Unexpected state");
        assert!(matches!(err, IntentError::InternalError(_)));
        assert!(err.to_string().contains("Unexpected state"));
    }

    #[test]
    fn test_intent_error_unknown_intent() {
        let err = IntentError::UnknownIntent("fake_intent".into());
        assert!(err.to_string().contains("fake_intent"));
    }

    #[test]
    fn test_intent_error_idempotency_conflict() {
        let err = IntentError::IdempotencyConflict("Token already used".into());
        assert!(err.to_string().contains("Token already used"));
    }

    // ========================================================================
    // intent_error_to_error_data Tests
    // ========================================================================

    #[test]
    fn test_intent_error_to_error_data_unknown_intent() {
        let err = IntentError::UnknownIntent("test".into());
        let error_data = intent_error_to_error_data(&err);
        assert!(error_data.message.contains("Unknown intent"));
        assert!(error_data.message.contains("test"));
        assert!(error_data.message.contains("plan_training"));
    }

    #[test]
    fn test_intent_error_to_error_data_validation() {
        let err = IntentError::validation("Invalid date");
        let error_data = intent_error_to_error_data(&err);
        assert!(error_data.message.contains("Invalid input"));
        assert!(error_data.message.contains("Invalid date"));
    }

    #[test]
    fn test_intent_error_to_error_data_idempotency() {
        let err = IntentError::IdempotencyConflict("Duplicate token".into());
        let error_data = intent_error_to_error_data(&err);
        assert!(error_data.message.contains("Idempotency conflict"));
        assert!(error_data.message.contains("Duplicate token"));
    }

    #[test]
    fn test_intent_error_to_error_data_api() {
        let err = IntentError::api("Network error");
        let error_data = intent_error_to_error_data(&err);
        assert!(error_data.message.contains("API error"));
        assert!(error_data.message.contains("Network error"));
    }

    #[test]
    fn test_intent_error_to_error_data_internal() {
        let err = IntentError::internal("Crash");
        let error_data = intent_error_to_error_data(&err);
        assert!(error_data.message.contains("Internal error"));
        assert!(error_data.message.contains("Crash"));
    }

    // ========================================================================
    // Existing Tests (kept for completeness)
    // ========================================================================

    #[test]
    fn test_content_block_creation() {
        let text = ContentBlock::text("Hello");
        assert!(matches!(text, ContentBlock::Text { .. }));
    }

    #[test]
    fn test_intent_output_builder() {
        let output = IntentOutput::markdown("# Test")
            .with_suggestions(vec!["Suggestion".into()])
            .with_next_actions(vec!["Action".into()]);
        assert_eq!(output.content.len(), 1);
        assert_eq!(output.suggestions.len(), 1);
    }

    #[tokio::test]
    async fn test_idempotency_cache() {
        let cache = IdempotencyCache::new(Duration::from_secs(1));
        let output = IntentOutput::markdown("Test");
        cache.set("test", &output).await;
        assert!(cache.get("test").await.is_some());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(cache.get("test").await.is_none());
    }

    // ========================================================================
    // IdempotencyEntry Tests
    // ========================================================================

    #[test]
    fn test_idempotency_entry_new() {
        let output = IntentOutput::markdown("Test");
        let ttl = Duration::from_secs(3600);
        let entry = IdempotencyEntry::new(output.clone(), ttl);

        assert_eq!(entry.result.content.len(), output.content.len());
        assert!(entry.created_at <= Local::now());
        assert!(entry.expires_at > Local::now());
    }

    #[test]
    fn test_idempotency_entry_is_expired() {
        let output = IntentOutput::markdown("Test");
        let short_ttl = Duration::from_millis(50);
        let entry = IdempotencyEntry::new(output, short_ttl);

        assert!(!entry.is_expired());
        std::thread::sleep(Duration::from_millis(100));
        assert!(entry.is_expired());
    }

    #[test]
    fn test_idempotency_entry_clone() {
        let output = IntentOutput::markdown("Test");
        let entry = IdempotencyEntry::new(output, Duration::from_secs(60));
        let cloned = entry.clone();

        assert_eq!(cloned.result.content.len(), entry.result.content.len());
        assert_eq!(cloned.created_at, entry.created_at);
        assert_eq!(cloned.expires_at, entry.expires_at);
    }

    // ========================================================================
    // IdempotencyCache Tests
    // ========================================================================

    #[tokio::test]
    async fn test_idempotency_cache_default() {
        let cache = IdempotencyCache::default();
        let output = IntentOutput::markdown("Test");
        cache.set("default_key", &output).await;
        assert!(cache.get("default_key").await.is_some());
    }

    #[tokio::test]
    async fn test_idempotency_cache_get_missing() {
        let cache = IdempotencyCache::new(Duration::from_secs(60));
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_idempotency_cache_set_with_ttl() {
        let cache = IdempotencyCache::new(Duration::from_secs(1));
        let output = IntentOutput::markdown("Test");
        cache
            .set_with_ttl("custom_ttl", &output, Duration::from_secs(60))
            .await;
        assert!(cache.get("custom_ttl").await.is_some());
    }

    #[tokio::test]
    async fn test_idempotency_cache_overwrite() {
        let cache = IdempotencyCache::new(Duration::from_secs(60));
        let output1 = IntentOutput::markdown("First");
        let output2 = IntentOutput::markdown("Second");

        cache.set("key", &output1).await;
        cache.set("key", &output2).await;

        let result = cache.get("key").await.unwrap();
        assert_eq!(result.content.len(), 1);
    }

    #[tokio::test]
    async fn test_idempotency_cache_concurrent_access() {
        let cache = Arc::new(IdempotencyCache::new(Duration::from_secs(60)));
        let mut handles = vec![];

        for i in 0..5 {
            let cache_clone = Arc::clone(&cache);
            let handle = tokio::spawn(async move {
                let output = IntentOutput::markdown(format!("Test {}", i));
                cache_clone.set(&format!("key_{}", i), &output).await;
                cache_clone.get(&format!("key_{}", i)).await
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_some());
        }
    }

    // ========================================================================
    // ToolDefinition Tests
    // ========================================================================

    #[test]
    fn test_tool_definition_new() {
        let schema = json!({"type": "object"});
        let tool = ToolDefinition::new("test_tool", "Test description", schema.clone());

        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, "Test description");
        assert_eq!(tool.input_schema, schema);
        assert!(tool.output_schema.is_none());
    }

    #[test]
    fn test_tool_definition_with_output_schema() {
        let input_schema = json!({"type": "object"});
        let output_schema = json!({"type": "array"});
        let tool =
            ToolDefinition::new("test", "desc", input_schema).with_output_schema(output_schema);

        assert!(tool.output_schema.is_some());
        assert_eq!(tool.output_schema.unwrap(), json!({"type": "array"}));
    }

    #[test]
    fn test_tool_definition_clone() {
        let schema = json!({"type": "object"});
        let tool = ToolDefinition::new("clone_test", "Clone me", schema);
        let cloned = tool.clone();

        assert_eq!(cloned.name, tool.name);
        assert_eq!(cloned.description, tool.description);
        assert_eq!(cloned.input_schema, tool.input_schema);
    }

    #[test]
    fn test_tool_definition_debug() {
        let schema = json!({"type": "object"});
        let tool = ToolDefinition::new("debug", "Debug test", schema);
        let debug = format!("{:?}", tool);

        assert!(debug.contains("ToolDefinition"));
        assert!(debug.contains("debug"));
    }

    // ========================================================================
    // standard_output_schema() Tests
    // ========================================================================

    #[test]
    fn test_standard_output_schema_structure() {
        let schema = standard_output_schema();

        assert!(schema.is_object());
        let obj = schema.as_object().unwrap();

        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("properties"));
        assert_eq!(obj.get("type").unwrap().as_str(), Some("object"));
    }

    #[test]
    fn test_standard_output_schema_content_property() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let content = props.get("content").unwrap();

        assert_eq!(content.get("type").unwrap().as_str(), Some("array"));
        assert!(content.get("description").is_some());
    }

    #[test]
    fn test_standard_output_schema_content_items() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let content = props.get("content").unwrap();
        let items = content.get("items").unwrap();

        let item_props = items.as_object().unwrap();
        assert!(item_props.contains_key("properties"));
        assert!(item_props.contains_key("required"));

        let required = item_props.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("type")));
    }

    #[test]
    fn test_standard_output_schema_suggestions() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let suggestions = props.get("suggestions").unwrap();

        assert_eq!(suggestions.get("type").unwrap().as_str(), Some("array"));
        assert!(suggestions.get("description").is_some());
    }

    #[test]
    fn test_standard_output_schema_next_actions() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let next_actions = props.get("next_actions").unwrap();

        assert_eq!(next_actions.get("type").unwrap().as_str(), Some("array"));
    }

    #[test]
    fn test_standard_output_schema_metadata() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let metadata = props.get("metadata").unwrap();

        assert_eq!(metadata.get("type").unwrap().as_str(), Some("object"));
        let meta_props = metadata.get("properties").unwrap().as_object().unwrap();

        assert!(meta_props.contains_key("has_more"));
        assert!(meta_props.contains_key("next_offset"));
        assert!(meta_props.contains_key("total_count"));
        assert!(meta_props.contains_key("events_created"));
        assert!(meta_props.contains_key("events_modified"));
        assert!(meta_props.contains_key("events_deleted"));
    }

    #[test]
    fn test_standard_output_schema_note() {
        let schema = standard_output_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let note = props.get("note").unwrap();

        assert_eq!(note.get("type").unwrap().as_str(), Some("string"));
    }

    #[test]
    fn test_standard_output_schema_required_fields() {
        let schema = standard_output_schema();
        let obj = schema.as_object().unwrap();
        let required = obj.get("required").unwrap().as_array().unwrap();

        assert!(required.iter().any(|v| v.as_str() == Some("content")));
    }

    #[test]
    fn test_standard_output_schema_serialization() {
        let schema = standard_output_schema();
        let serialized = serde_json::to_string(&schema);
        assert!(serialized.is_ok());

        let deserialized: Value = serde_json::from_str(&serialized.unwrap()).unwrap();
        assert_eq!(deserialized, schema);
    }

    // ========================================================================
    // intent_output_to_call_tool_result() Tests
    // ========================================================================

    #[test]
    fn test_intent_output_to_call_tool_result_text() {
        let output = IntentOutput::new(vec![ContentBlock::text("Hello")]);
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_markdown() {
        let output = IntentOutput::markdown("# Header");
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_table() {
        let headers = vec!["A".into(), "B".into()];
        let rows = vec![vec!["1".into(), "2".into()]];
        let output = IntentOutput::new(vec![ContentBlock::table(headers, rows)]);
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        let table = result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .expect("structured table block is present");
        assert_eq!(table.get("type").and_then(Value::as_str), Some("table"));
    }

    #[test]
    fn test_intent_output_to_call_tool_result_with_suggestions() {
        let output = IntentOutput::markdown("Test").with_suggestions(vec!["Suggestion 1".into()]);
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("suggestions"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_with_next_actions() {
        let output = IntentOutput::markdown("Test").with_next_actions(vec!["Action 1".into()]);
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("next_actions"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_with_note() {
        let output = IntentOutput::markdown("Test").with_note("Test note");
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("note"))
                .and_then(Value::as_str),
            Some("Test note")
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_multiple_content_blocks() {
        let output = IntentOutput::new(vec![
            ContentBlock::text("Text"),
            ContentBlock::markdown("**Bold**"),
            ContentBlock::table(vec!["H1".into()], vec![vec!["R1".into()]]),
        ]);
        let result = intent_output_to_call_tool_result(&output);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.is_empty());
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(3)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_complex_output() {
        let meta = OutputMetadata {
            events_created: Some(5),
            total_count: Some(10),
            ..Default::default()
        };

        let output = IntentOutput::markdown("# Report")
            .with_suggestions(vec!["Try this".into()])
            .with_next_actions(vec!["Do that".into()])
            .with_metadata(meta)
            .with_note("Cached");

        let result = intent_output_to_call_tool_result(&output);
        assert!(result.is_ok());
        let result = result.unwrap();

        assert!(result.content.is_empty());
        assert!(result.structured_content.is_some());
    }

    #[test]
    fn test_intent_output_to_call_tool_result_includes_structured_content() {
        let output = IntentOutput::new(vec![
            ContentBlock::markdown("## Analysis: Recovery Run"),
            ContentBlock::table(
                vec!["Metric".into(), "Value".into()],
                vec![vec!["Distance".into(), "7.04 km".into()]],
            ),
        ])
        .with_suggestions(vec!["Average heart rate held steady.".into()])
        .with_next_actions(vec![
            "To compare with similar workouts: compare_periods".into(),
        ]);

        let result = intent_output_to_call_tool_result(&output).expect("conversion succeeds");
        let json = serde_json::to_value(&result).expect("result serializes");

        let structured = json
            .get("structuredContent")
            .and_then(Value::as_object)
            .expect("tool result should include structuredContent when output schema is published");

        let content = structured
            .get("content")
            .and_then(Value::as_array)
            .expect("structuredContent should expose content blocks");
        assert_eq!(content.len(), 2);
        assert_eq!(
            structured
                .get("suggestions")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            structured
                .get("next_actions")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_structured_content_preserves_metadata() {
        let output = IntentOutput::markdown("## Period Analysis")
            .with_metadata(OutputMetadata {
                has_more: Some(true),
                next_offset: Some("page-2".into()),
                total_count: Some(42),
                ..Default::default()
            })
            .with_note("Cached result");

        let result = intent_output_to_call_tool_result(&output).expect("conversion succeeds");
        let json = serde_json::to_value(&result).expect("result serializes");
        let structured = json
            .get("structuredContent")
            .and_then(Value::as_object)
            .expect("tool result should expose structuredContent");
        let metadata = structured
            .get("metadata")
            .and_then(Value::as_object)
            .expect("structuredContent should preserve metadata");

        assert_eq!(
            metadata.get("has_more").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            metadata.get("next_offset").and_then(Value::as_str),
            Some("page-2")
        );
        assert_eq!(
            metadata.get("total_count").and_then(Value::as_u64),
            Some(42)
        );
        assert_eq!(
            structured.get("note").and_then(Value::as_str),
            Some("Cached result")
        );
    }

    #[test]
    fn test_intent_output_to_call_tool_result_uses_schema_only_payload() {
        let output = IntentOutput::new(vec![ContentBlock::text("compact structured result")]);

        let result = intent_output_to_call_tool_result(&output).expect("conversion succeeds");

        assert!(
            result.content.is_empty(),
            "intent tool results should avoid duplicate text content and rely on structuredContent only"
        );
        assert!(result.structured_content.is_some());
        assert_eq!(result.is_error, Some(false));
    }

    // ========================================================================
    // IntentHandler Trait Tests (via mock implementation)
    // ========================================================================

    struct MockIntentHandler {
        name: &'static str,
        description: &'static str,
        schema: Value,
    }

    #[async_trait::async_trait]
    impl IntentHandler for MockIntentHandler {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> &'static str {
            self.description
        }

        fn input_schema(&self) -> Value {
            self.schema.clone()
        }

        async fn execute(
            &self,
            _input: Value,
            _client: Arc<dyn IntervalsClient>,
            _idempotency_cache: Option<&IdempotencyCache>,
        ) -> Result<IntentOutput, IntentError> {
            Ok(IntentOutput::markdown("Mock result"))
        }

        fn requires_idempotency_token(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_intent_handler_trait_methods() {
        let handler = MockIntentHandler {
            name: "mock_handler",
            description: "Mock handler for testing",
            schema: json!({"type": "object"}),
        };

        assert_eq!(handler.name(), "mock_handler");
        assert_eq!(handler.description(), "Mock handler for testing");
        assert_eq!(handler.input_schema(), json!({"type": "object"}));
        assert!(handler.requires_idempotency_token());
    }

    #[tokio::test]
    async fn test_intent_handler_execute() {
        let handler = MockIntentHandler {
            name: "mock",
            description: "test",
            schema: json!({}),
        };

        // Note: We can't easily test execute() without a real client mock
        // This test verifies the trait compiles correctly
        assert_eq!(handler.name(), "mock");
    }

    #[test]
    fn test_intent_handler_extract_idempotency_token() {
        let handler = MockIntentHandler {
            name: "test",
            description: "test",
            schema: json!({}),
        };

        let input_with_token = json!({"idempotency_token": "abc123"});
        let token = handler.extract_idempotency_token(&input_with_token);
        assert_eq!(token, Some("abc123".to_string()));

        let input_without_token = json!({"other": "value"});
        let token = handler.extract_idempotency_token(&input_without_token);
        assert_eq!(token, None);

        let input_null_token = json!({"idempotency_token": null});
        let token = handler.extract_idempotency_token(&input_null_token);
        assert_eq!(token, None);
    }

    #[test]
    fn test_intent_handler_extract_idempotency_token_non_string() {
        let handler = MockIntentHandler {
            name: "test",
            description: "test",
            schema: json!({}),
        };

        let input_number_token = json!({"idempotency_token": 123});
        let token = handler.extract_idempotency_token(&input_number_token);
        assert_eq!(token, None);
    }
}

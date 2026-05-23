/// Intent-driven architecture layer
pub mod error;
pub mod handlers;
pub mod idempotency;
pub mod router;
pub mod types;
pub mod utils;
pub mod validator;

pub use error::ErrorGuidance;
pub use idempotency::IdempotencyMiddleware;
pub use router::IntentRouter;
pub use types::{
    ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput, OutputMetadata,
    ToolDefinition, intent_error_to_error_data, intent_output_to_call_tool_result,
};
pub use utils::*;
pub use validator::Validator;

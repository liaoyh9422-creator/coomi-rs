use crate::AgentEvent;
use crate::AgentObserver;
use crate::ModelResponse;
use crate::ModelStreamObserver;
use crate::upgrades::policy::DEFAULT_UPGRADE_POLICY;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const EMPTY_RESPONSE_MAX_RETRIES: u8 = DEFAULT_UPGRADE_POLICY.empty_response_max_retries;
pub const REASONING_ONLY_MAX_RETRIES: u8 = DEFAULT_UPGRADE_POLICY.reasoning_only_max_retries;

#[derive(Clone)]
pub struct RecoveryStreamObserver<'a> {
    observer: &'a dyn AgentObserver,
    saw_reasoning: Arc<AtomicBool>,
}

impl<'a> RecoveryStreamObserver<'a> {
    pub fn new(observer: &'a dyn AgentObserver) -> Self {
        Self {
            observer,
            saw_reasoning: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn saw_reasoning(&self) -> bool {
        self.saw_reasoning.load(Ordering::Acquire)
    }
}

impl ModelStreamObserver for RecoveryStreamObserver<'_> {
    fn on_text_delta(&self, delta: &str) {
        self.observer
            .on_event(&AgentEvent::TextDelta(delta.to_owned()));
    }

    fn on_reasoning_delta(&self, delta: &str) {
        self.saw_reasoning.store(true, Ordering::Release);
        self.observer
            .on_event(&AgentEvent::ReasoningDelta(delta.to_owned()));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseRecovery {
    Continue,
    RetryEmpty,
    RetryReasoningOnly,
}

pub fn classify_response(response: &ModelResponse, saw_reasoning: bool) -> ResponseRecovery {
    if !response.tool_calls.is_empty() || !response.content.trim().is_empty() {
        return ResponseRecovery::Continue;
    }
    if saw_reasoning {
        ResponseRecovery::RetryReasoningOnly
    } else {
        ResponseRecovery::RetryEmpty
    }
}

pub fn retry_instruction(recovery: ResponseRecovery, attempt: u8, max: u8) -> Option<String> {
    match recovery {
        ResponseRecovery::Continue => None,
        ResponseRecovery::RetryEmpty => Some(format!(
            "The previous assistant response was empty. Retry with either a direct answer or a valid tool call. Attempt {attempt}/{max}."
        )),
        ResponseRecovery::RetryReasoningOnly => Some(format!(
            "The previous assistant response only contained hidden reasoning. Retry with visible answer text or a valid tool call. Attempt {attempt}/{max}."
        )),
    }
}

pub fn is_context_window_error(error: &anyhow::Error) -> bool {
    let value = format!("{error:#}").to_ascii_lowercase();
    value.contains("context_window_exceeded")
        || value.contains("context length")
        || value.contains("maximum context")
        || value.contains("too many tokens")
        || value.contains("context too long")
        || value.contains("max context")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCall;
    use serde_json::json;

    #[test]
    fn continues_when_response_has_text() {
        let response = ModelResponse {
            content: "done".into(),
            ..Default::default()
        };
        assert_eq!(
            classify_response(&response, true),
            ResponseRecovery::Continue
        );
    }

    #[test]
    fn continues_when_response_has_tool_calls() {
        let response = ModelResponse {
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "README.md"}),
            }],
            ..Default::default()
        };
        assert_eq!(
            classify_response(&response, false),
            ResponseRecovery::Continue
        );
    }

    #[test]
    fn retries_empty_response_without_reasoning() {
        assert_eq!(
            classify_response(&ModelResponse::default(), false),
            ResponseRecovery::RetryEmpty
        );
    }

    #[test]
    fn retries_reasoning_only_response() {
        assert_eq!(
            classify_response(&ModelResponse::default(), true),
            ResponseRecovery::RetryReasoningOnly
        );
    }

    #[test]
    fn detects_common_context_window_errors() {
        let error = anyhow::anyhow!("maximum context length exceeded");
        assert!(is_context_window_error(&error));
    }
}

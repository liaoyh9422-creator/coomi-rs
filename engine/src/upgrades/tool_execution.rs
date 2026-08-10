use crate::AgentEvent;
use crate::AgentObserver;
use crate::ApprovalHandler;
use crate::ChatMessage;
use crate::Session;
use crate::ToolCall;
use crate::ToolResult;
use crate::ToolRuntime;
use crate::upgrades::tool_policy::{is_parallel_safe, trim_tool_result};
use crate::upgrades::tool_validation::ToolValidator;
use futures_util::future::join_all;
use std::collections::HashSet;

pub async fn execute_tool_calls(
    session: &mut Session,
    calls: Vec<ToolCall>,
    tools: &dyn ToolRuntime,
    approval: &dyn ApprovalHandler,
    observer: &dyn AgentObserver,
) {
    let calls = normalize_tool_calls(calls);
    let validator = ToolValidator::new(tools.specs());
    let results = if calls.iter().all(is_parallel_safe) {
        execute_parallel_safe_calls(&calls, tools, approval, observer, &validator).await
    } else {
        execute_sequential_calls(&calls, tools, approval, observer, &validator).await
    };
    for (call, result) in calls.iter().zip(results) {
        apply_tool_result(session, call, result, observer);
    }
}

fn normalize_tool_calls(calls: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(calls.len());
    for call in calls {
        let key = format!("{}\u{1f}{}", call.name.trim(), call.arguments);
        if seen.insert(key) {
            normalized.push(call);
        }
    }
    normalized
}

async fn execute_sequential_calls(
    calls: &[ToolCall],
    tools: &dyn ToolRuntime,
    approval: &dyn ApprovalHandler,
    observer: &dyn AgentObserver,
    validator: &ToolValidator,
) -> Vec<ToolResult> {
    let mut results = Vec::with_capacity(calls.len());
    for call in calls {
        observer.on_event(&AgentEvent::ToolStarted(call.clone()));
        results.push(execute_one_tool(call, tools, approval, validator).await);
    }
    results
}

async fn execute_parallel_safe_calls(
    calls: &[ToolCall],
    tools: &dyn ToolRuntime,
    approval: &dyn ApprovalHandler,
    observer: &dyn AgentObserver,
    validator: &ToolValidator,
) -> Vec<ToolResult> {
    for call in calls {
        observer.on_event(&AgentEvent::ToolStarted(call.clone()));
    }
    join_all(
        calls
            .iter()
            .map(|call| execute_one_tool(call, tools, approval, validator)),
    )
    .await
}

async fn execute_one_tool(
    call: &ToolCall,
    tools: &dyn ToolRuntime,
    approval: &dyn ApprovalHandler,
    validator: &ToolValidator,
) -> ToolResult {
    if let Err(error) = validator.validate(call) {
        return ToolResult::error(format!(
            "Invalid tool call: {error}. Please retry with corrected arguments."
        ));
    }
    trim_tool_result(tools.call(call, approval).await)
}

fn apply_tool_result(
    session: &mut Session,
    call: &ToolCall,
    result: ToolResult,
    observer: &dyn AgentObserver,
) {
    if let Some(plan) = result.plan.clone() {
        session.plan = Some(plan.clone());
        observer.on_event(&AgentEvent::PlanUpdated(plan));
    }
    if let Some(loop_state) = result.loop_state.clone() {
        session.loop_state = Some(loop_state.clone());
        observer.on_event(&AgentEvent::LoopUpdated(loop_state));
    }
    observer.on_event(&AgentEvent::ToolFinished {
        call: call.clone(),
        result: result.clone(),
    });
    let status = if result.success { "success" } else { "error" };
    let mut tool_message =
        ChatMessage::tool(call.id.clone(), format!("{status}: {}", result.output));
    tool_message.images = result.images.clone();
    session.messages.push(tool_message);
    if let Some(context) = result.additional_context
        && !context.trim().is_empty()
    {
        session.messages.push(ChatMessage::internal_user(context));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoopObserver;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    struct Approve;

    #[async_trait]
    impl ApprovalHandler for Approve {
        async fn approve(&self, _call: &ToolCall, _reason: &str) -> bool {
            true
        }
    }

    struct CountingTools {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ToolRuntime for CountingTools {
        fn specs(&self) -> Vec<crate::ToolSpec> {
            vec![crate::ToolSpec {
                name: "echo".into(),
                description: "echo".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                    "required": ["value"],
                    "additionalProperties": false
                }),
            }]
        }

        async fn call(&self, call: &ToolCall, _approval: &dyn ApprovalHandler) -> ToolResult {
            self.calls.lock().expect("lock calls").push(call.id.clone());
            ToolResult::success(call.name.clone())
        }
    }

    #[tokio::test]
    async fn deduplicates_identical_tool_calls() -> Result<()> {
        let tools = CountingTools {
            calls: Mutex::new(Vec::new()),
        };
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        let call = ToolCall {
            id: "one".into(),
            name: "echo".into(),
            arguments: json!({"value": "same"}),
        };
        execute_tool_calls(
            &mut session,
            vec![
                call.clone(),
                ToolCall {
                    id: "two".into(),
                    ..call
                },
            ],
            &tools,
            &Approve,
            &NoopObserver,
        )
        .await;
        assert_eq!(tools.calls.lock().expect("lock calls").len(), 1);
        assert_eq!(session.messages.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unknown_tool_before_runtime_dispatch() -> Result<()> {
        let tools = CountingTools {
            calls: Mutex::new(Vec::new()),
        };
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        execute_tool_calls(
            &mut session,
            vec![ToolCall {
                id: "missing".into(),
                name: "missing_tool".into(),
                arguments: json!({}),
            }],
            &tools,
            &Approve,
            &NoopObserver,
        )
        .await;
        assert!(tools.calls.lock().expect("lock calls").is_empty());
        assert!(
            session.messages[0]
                .content
                .contains("unknown tool `missing_tool`")
        );
        assert!(session.messages[0].content.contains("echo"));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_missing_required_arguments_before_runtime_dispatch() -> Result<()> {
        let tools = CountingTools {
            calls: Mutex::new(Vec::new()),
        };
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        execute_tool_calls(
            &mut session,
            vec![ToolCall {
                id: "missing-path".into(),
                name: "echo".into(),
                arguments: json!({}),
            }],
            &tools,
            &Approve,
            &NoopObserver,
        )
        .await;
        assert!(tools.calls.lock().expect("lock calls").is_empty());
        assert!(
            session.messages[0]
                .content
                .contains("missing required argument")
        );
        assert!(session.messages[0].content.contains("value"));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_non_object_arguments_before_runtime_dispatch() -> Result<()> {
        let tools = CountingTools {
            calls: Mutex::new(Vec::new()),
        };
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        execute_tool_calls(
            &mut session,
            vec![ToolCall {
                id: "bad".into(),
                name: "echo".into(),
                arguments: json!("not-object"),
            }],
            &tools,
            &Approve,
            &NoopObserver,
        )
        .await;
        assert!(tools.calls.lock().expect("lock calls").is_empty());
        assert!(
            session.messages[0]
                .content
                .contains("arguments must be a JSON object")
        );
        Ok(())
    }

    struct SlowReadTools {
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    #[async_trait]
    impl ToolRuntime for SlowReadTools {
        fn specs(&self) -> Vec<crate::ToolSpec> {
            vec![crate::ToolSpec {
                name: "read_file".into(),
                description: "read".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
            }]
        }

        async fn call(&self, call: &ToolCall, _approval: &dyn ApprovalHandler) -> ToolResult {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            ToolResult::success(call.id.clone())
        }
    }

    #[tokio::test]
    async fn runs_parallel_safe_tools_concurrently() -> Result<()> {
        let tools = SlowReadTools {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
        };
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        let calls = (0..3)
            .map(|index| ToolCall {
                id: format!("read-{index}"),
                name: "read_file".into(),
                arguments: json!({"path": format!("file-{index}.txt")}),
            })
            .collect();
        let started = Instant::now();
        execute_tool_calls(&mut session, calls, &tools, &Approve, &NoopObserver).await;
        assert_eq!(session.messages.len(), 3);
        assert!(tools.max_active.load(Ordering::SeqCst) > 1);
        assert!(started.elapsed() < Duration::from_millis(110));
        Ok(())
    }

    struct LargeOutputTools;

    #[async_trait]
    impl ToolRuntime for LargeOutputTools {
        fn specs(&self) -> Vec<crate::ToolSpec> {
            vec![crate::ToolSpec {
                name: "read_file".into(),
                description: "read".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
            }]
        }

        async fn call(&self, _call: &ToolCall, _approval: &dyn ApprovalHandler) -> ToolResult {
            ToolResult::success("x".repeat(60_000))
        }
    }

    #[tokio::test]
    async fn trims_large_tool_outputs_before_history_insert() -> Result<()> {
        let mut session = Session::new("mock", "model", PathBuf::from("."));
        execute_tool_calls(
            &mut session,
            vec![ToolCall {
                id: "large".into(),
                name: "read_file".into(),
                arguments: json!({"path": "large.txt"}),
            }],
            &LargeOutputTools,
            &Approve,
            &NoopObserver,
        )
        .await;
        assert!(
            session.messages[0]
                .content
                .contains("[tool output truncated]")
        );
        assert!(session.messages[0].content.len() < 49_000);
        Ok(())
    }
}

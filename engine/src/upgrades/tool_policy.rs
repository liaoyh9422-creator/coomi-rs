use crate::ToolCall;
use crate::ToolResult;
use crate::upgrades::policy::DEFAULT_UPGRADE_POLICY;

const MAX_TOOL_OUTPUT_BYTES: usize = DEFAULT_UPGRADE_POLICY.max_tool_output_bytes;

pub fn is_parallel_safe(call: &ToolCall) -> bool {
    matches!(
        call.name.as_str(),
        "read_file"
            | "list_dir"
            | "grep_files"
            | "search"
            | "web_search"
            | "fetch"
            | "view_image"
            | "show_image"
            | "list_skills"
            | "read_skill"
            | "memory_list"
            | "memory_read"
            | "memory_search"
            | "get_loop"
    )
}

pub fn trim_tool_result(mut result: ToolResult) -> ToolResult {
    if result.output.len() <= MAX_TOOL_OUTPUT_BYTES {
        return result;
    }
    truncate_at_char_boundary(&mut result.output, MAX_TOOL_OUTPUT_BYTES);
    result.output.push_str("\n[tool output truncated]");
    result
}

fn truncate_at_char_boundary(value: &mut String, max_bytes: usize) {
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_utf8_boundary_when_trimming() {
        let result = ToolResult::success(format!("{}终", "a".repeat(MAX_TOOL_OUTPUT_BYTES)));
        let result = trim_tool_result(result);
        assert!(result.output.ends_with("[tool output truncated]"));
        assert!(result.output.is_char_boundary(result.output.len()));
    }

    #[test]
    fn classifies_read_only_tools_as_parallel_safe() {
        let call = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "README.md"}),
        };
        assert!(is_parallel_safe(&call));
    }

    #[test]
    fn keeps_mutating_tools_sequential() {
        let call = ToolCall {
            id: "1".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({"path": "a", "content": "b"}),
        };
        assert!(!is_parallel_safe(&call));
    }
}

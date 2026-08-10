use crate::ToolCall;
use crate::ToolSpec;
use serde_json::Map;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct ToolValidator {
    specs: BTreeMap<String, ToolSpec>,
}

impl ToolValidator {
    pub fn new(specs: Vec<ToolSpec>) -> Self {
        Self {
            specs: specs
                .into_iter()
                .map(|spec| (spec.name.clone(), spec))
                .collect(),
        }
    }

    pub fn validate(&self, call: &ToolCall) -> Result<(), String> {
        if call.name.trim().is_empty() {
            return Err("tool call is missing a name".into());
        }
        let Some(spec) = self.specs.get(&call.name) else {
            return Err(format!(
                "unknown tool `{}`. Available tools: {}",
                call.name,
                self.available_tools()
            ));
        };
        let Some(args) = call.arguments.as_object() else {
            return Err(
                "tool arguments must be a JSON object; use {} when no arguments are required"
                    .into(),
            );
        };
        validate_required(args, &spec.parameters)?;
        validate_additional_properties(args, &spec.parameters)?;
        validate_property_types(args, &spec.parameters)?;
        Ok(())
    }

    fn available_tools(&self) -> String {
        self.specs.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

fn validate_required(args: &Map<String, Value>, schema: &Value) -> Result<(), String> {
    let missing = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| !args.contains_key(*name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "missing required argument(s): {}",
            missing.join(", ")
        ))
    }
}

fn validate_additional_properties(args: &Map<String, Value>, schema: &Value) -> Result<(), String> {
    if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Ok(());
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    let extras = args
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if extras.is_empty() {
        Ok(())
    } else {
        Err(format!("unknown argument(s): {}", extras.join(", ")))
    }
}

fn validate_property_types(args: &Map<String, Value>, schema: &Value) -> Result<(), String> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    let mut errors = Vec::new();
    for (name, value) in args {
        let Some(property) = properties.get(name) else {
            continue;
        };
        let Some(expected) = property.get("type").and_then(Value::as_str) else {
            continue;
        };
        if !matches_json_type(value, expected) {
            errors.push(format!(
                "{name} must be {expected}, got {}",
                json_type(value)
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn matches_json_type(value: &Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn validator() -> ToolValidator {
        ToolValidator::new(vec![ToolSpec {
            name: "read_file".into(),
            description: "read".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "limit": {"type": "integer"}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }])
    }

    #[test]
    fn rejects_unknown_tool_with_available_list() {
        let error = validator()
            .validate(&ToolCall {
                id: "1".into(),
                name: "missing".into(),
                arguments: json!({}),
            })
            .expect_err("unknown tool");
        assert!(error.contains("unknown tool `missing`"));
        assert!(error.contains("read_file"));
    }

    #[test]
    fn rejects_missing_required_argument() {
        let error = validator()
            .validate(&ToolCall {
                id: "1".into(),
                name: "read_file".into(),
                arguments: json!({"limit": 10}),
            })
            .expect_err("missing argument");
        assert!(error.contains("missing required argument"));
        assert!(error.contains("path"));
    }

    #[test]
    fn rejects_extra_argument_when_schema_disallows_it() {
        let error = validator()
            .validate(&ToolCall {
                id: "1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "README.md", "extra": true}),
            })
            .expect_err("extra argument");
        assert!(error.contains("unknown argument"));
        assert!(error.contains("extra"));
    }

    #[test]
    fn rejects_obvious_type_mismatch() {
        let error = validator()
            .validate(&ToolCall {
                id: "1".into(),
                name: "read_file".into(),
                arguments: json!({"path": 10}),
            })
            .expect_err("type mismatch");
        assert!(error.contains("path must be string"));
    }

    #[test]
    fn accepts_valid_arguments() {
        validator()
            .validate(&ToolCall {
                id: "1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "README.md", "limit": 20}),
            })
            .expect("valid call");
    }
}

use reqwest::StatusCode;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderErrorKind {
    ContextWindow,
    RateLimit,
    Auth,
    Server,
    BadRequest,
    Unknown,
}

impl ProviderErrorKind {
    fn label(self) -> &'static str {
        match self {
            Self::ContextWindow => "context_window",
            Self::RateLimit => "rate_limit",
            Self::Auth => "auth",
            Self::Server => "server",
            Self::BadRequest => "bad_request",
            Self::Unknown => "unknown",
        }
    }
}

pub fn provider_http_error(status: StatusCode, body: &str) -> String {
    let detail = error_detail(body);
    let kind = classify_provider_error(status, &detail);
    format!(
        "provider_error={}: HTTP {}: {}",
        kind.label(),
        status.as_u16(),
        detail
    )
}

pub fn classify_provider_error(status: StatusCode, detail: &str) -> ProviderErrorKind {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("context_window_exceeded")
        || lower.contains("context length")
        || lower.contains("maximum context")
        || lower.contains("too many tokens")
        || lower.contains("context too long")
        || lower.contains("max context")
    {
        ProviderErrorKind::ContextWindow
    } else if status == StatusCode::TOO_MANY_REQUESTS || lower.contains("rate limit") {
        ProviderErrorKind::RateLimit
    } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || lower.contains("invalid api key")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
    {
        ProviderErrorKind::Auth
    } else if status.is_server_error() {
        ProviderErrorKind::Server
    } else if status.is_client_error() {
        ProviderErrorKind::BadRequest
    } else {
        ProviderErrorKind::Unknown
    }
}

fn error_detail(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let text = parsed
        .as_ref()
        .and_then(extract_json_error_message)
        .unwrap_or(body)
        .chars()
        .take(800)
        .collect::<String>();
    if text.trim().is_empty() {
        "<empty response body>".into()
    } else {
        text
    }
}

fn extract_json_error_message(value: &Value) -> Option<&str> {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error").and_then(Value::as_str))
        .or_else(|| value.pointer("/message").and_then(Value::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_context_window_error_from_body() {
        let message = provider_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"maximum context length exceeded"}}"#,
        );
        assert!(message.contains("provider_error=context_window"));
        assert!(message.contains("maximum context length exceeded"));
    }

    #[test]
    fn classifies_rate_limit_from_status() {
        let message = provider_http_error(StatusCode::TOO_MANY_REQUESTS, "slow down");
        assert!(message.contains("provider_error=rate_limit"));
    }

    #[test]
    fn classifies_auth_from_status() {
        let message = provider_http_error(StatusCode::UNAUTHORIZED, "bad key");
        assert!(message.contains("provider_error=auth"));
    }

    #[test]
    fn classifies_server_error_from_status() {
        let message = provider_http_error(StatusCode::BAD_GATEWAY, "upstream failed");
        assert!(message.contains("provider_error=server"));
    }
}

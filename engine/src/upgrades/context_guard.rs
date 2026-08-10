use crate::ContextStatus;
use crate::upgrades::policy::DEFAULT_UPGRADE_POLICY;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPressure {
    Normal,
    Caution,
    Warning,
    Critical,
}

impl ContextPressure {
    pub fn should_emit(self) -> bool {
        !matches!(self, Self::Normal)
    }
}

pub fn classify_context_pressure(status: &ContextStatus) -> ContextPressure {
    let effective_percent = percentage(status.used_tokens, status.effective_context_window);
    let compact_percent = percentage(
        status.auto_compact_scope_tokens,
        status.auto_compact_token_limit.max(1),
    );
    let pressure = effective_percent.max(compact_percent);
    if status.used_tokens >= status.effective_context_window
        || status.auto_compact_scope_tokens >= status.auto_compact_token_limit
        || pressure >= DEFAULT_UPGRADE_POLICY.context_critical_percent
    {
        ContextPressure::Critical
    } else if pressure >= DEFAULT_UPGRADE_POLICY.context_warning_percent {
        ContextPressure::Warning
    } else if pressure >= DEFAULT_UPGRADE_POLICY.context_caution_percent {
        ContextPressure::Caution
    } else {
        ContextPressure::Normal
    }
}

pub fn context_pressure_message(status: &ContextStatus) -> Option<String> {
    let pressure = classify_context_pressure(status);
    if !pressure.should_emit() {
        return None;
    }
    let label = match pressure {
        ContextPressure::Normal => return None,
        ContextPressure::Caution => "Context pressure rising",
        ContextPressure::Warning => "Context pressure high",
        ContextPressure::Critical => "Context compaction threshold reached",
    };
    Some(format!(
        "{label}: {} used tokens, {} remaining, {}% of effective window, {}% of auto-compact scope.",
        status.used_tokens,
        status.remaining_tokens,
        percentage(status.used_tokens, status.effective_context_window),
        percentage(
            status.auto_compact_scope_tokens,
            status.auto_compact_token_limit.max(1)
        )
    ))
}

fn percentage(value: u64, total: u64) -> u8 {
    if total == 0 {
        return 100;
    }
    u8::try_from(value.saturating_mul(100).saturating_div(total).min(100)).unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(used: u64, effective: u64, scoped: u64, compact_limit: u64) -> ContextStatus {
        ContextStatus {
            used_tokens: used,
            context_window: effective,
            effective_context_window: effective,
            auto_compact_token_limit: compact_limit,
            remaining_tokens: effective.saturating_sub(used),
            used_percent: 0,
            remaining_percent: 0,
            auto_compact_scope_tokens: scoped,
            compaction_count: 0,
        }
    }

    #[test]
    fn classifies_normal_pressure() {
        assert_eq!(
            classify_context_pressure(&status(100, 1_000, 100, 1_000)),
            ContextPressure::Normal
        );
    }

    #[test]
    fn classifies_caution_pressure() {
        assert_eq!(
            classify_context_pressure(&status(810, 1_000, 100, 1_000)),
            ContextPressure::Caution
        );
    }

    #[test]
    fn classifies_warning_pressure_from_compaction_scope() {
        assert_eq!(
            classify_context_pressure(&status(100, 1_000, 910, 1_000)),
            ContextPressure::Warning
        );
    }

    #[test]
    fn classifies_critical_at_compaction_limit() {
        assert_eq!(
            classify_context_pressure(&status(100, 1_000, 1_000, 1_000)),
            ContextPressure::Critical
        );
    }

    #[test]
    fn formats_pressure_message() {
        let message =
            context_pressure_message(&status(900, 1_000, 900, 1_000)).expect("pressure message");
        assert!(message.contains("Context pressure high"));
        assert!(message.contains("900 used tokens"));
    }
}

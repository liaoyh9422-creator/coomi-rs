#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpgradePolicy {
    pub empty_response_max_retries: u8,
    pub reasoning_only_max_retries: u8,
    pub max_tool_output_bytes: usize,
    pub context_caution_percent: u8,
    pub context_warning_percent: u8,
    pub context_critical_percent: u8,
}

impl Default for UpgradePolicy {
    fn default() -> Self {
        Self {
            empty_response_max_retries: 2,
            reasoning_only_max_retries: 2,
            max_tool_output_bytes: 48_000,
            context_caution_percent: 80,
            context_warning_percent: 90,
            context_critical_percent: 98,
        }
    }
}

pub const DEFAULT_UPGRADE_POLICY: UpgradePolicy = UpgradePolicy {
    empty_response_max_retries: 2,
    reasoning_only_max_retries: 2,
    max_tool_output_bytes: 48_000,
    context_caution_percent: 80,
    context_warning_percent: 90,
    context_critical_percent: 98,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_const_policy() {
        assert_eq!(UpgradePolicy::default(), DEFAULT_UPGRADE_POLICY);
    }
}

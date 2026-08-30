use super::llm_usage::LlmTokenUsage;
use crate::error::{InfraError, InfraResult};
use std::sync::Mutex;

pub const DEFAULT_LLM_MAX_ATTEMPTS: u64 = 100;
pub const DEFAULT_LLM_MAX_TOKENS: u64 = 1_000_000;
pub(crate) const PROVIDER_MAX_OUTPUT_TOKENS: u64 = 4_096;
const CHAT_FRAMING_TOKEN_CEILING: u64 = 1_024;

#[derive(Debug)]
pub struct LlmRunBudget {
    max_attempts: u64,
    max_tokens: u64,
    state: Mutex<LlmRunBudgetState>,
}

#[derive(Debug, Default)]
struct LlmRunBudgetState {
    attempts_started: u64,
    charged_tokens: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LlmBudgetReservation {
    token_ceiling: u64,
}

impl Default for LlmRunBudget {
    fn default() -> Self {
        Self::with_limits(DEFAULT_LLM_MAX_ATTEMPTS, DEFAULT_LLM_MAX_TOKENS)
    }
}

impl LlmRunBudget {
    fn with_limits(max_attempts: u64, max_tokens: u64) -> Self {
        Self {
            max_attempts,
            max_tokens,
            state: Mutex::new(LlmRunBudgetState::default()),
        }
    }

    pub(crate) fn reserve(&self, prompt: &str, system: &str) -> InfraResult<LlmBudgetReservation> {
        let token_ceiling = request_token_ceiling(prompt, system);
        let mut state = self.state.lock().map_err(|_| {
            InfraError::LlmRequest("LLM run budget state lock poisoned".to_string())
        })?;
        if state.attempts_started >= self.max_attempts {
            return Err(InfraError::LlmBudgetExceeded {
                resource: "provider_attempts",
                limit: self.max_attempts,
                used: state.attempts_started,
                requested: 1,
            });
        }
        if state.charged_tokens.saturating_add(token_ceiling) > self.max_tokens {
            return Err(InfraError::LlmBudgetExceeded {
                resource: "provider_tokens",
                limit: self.max_tokens,
                used: state.charged_tokens,
                requested: token_ceiling,
            });
        }
        state.attempts_started = state.attempts_started.saturating_add(1);
        state.charged_tokens = state.charged_tokens.saturating_add(token_ceiling);
        Ok(LlmBudgetReservation { token_ceiling })
    }

    pub(crate) fn settle(
        &self,
        reservation: LlmBudgetReservation,
        usage: Option<&LlmTokenUsage>,
    ) -> InfraResult<()> {
        let Some(exact_tokens) = usage.and_then(|usage| usage.total_tokens) else {
            return Ok(());
        };
        let mut state = self.state.lock().map_err(|_| {
            InfraError::LlmRequest("LLM run budget state lock poisoned".to_string())
        })?;
        state.charged_tokens = state
            .charged_tokens
            .saturating_sub(reservation.token_ceiling)
            .saturating_add(exact_tokens);
        Ok(())
    }
}

fn request_token_ceiling(prompt: &str, system: &str) -> u64 {
    (prompt.len() as u64)
        .saturating_add(system.len() as u64)
        .saturating_add(PROVIDER_MAX_OUTPUT_TOKENS)
        .saturating_add(CHAT_FRAMING_TOKEN_CEILING)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(total_tokens: u64) -> LlmTokenUsage {
        LlmTokenUsage {
            total_tokens: Some(total_tokens),
            ..LlmTokenUsage::default()
        }
    }

    #[test]
    fn attempt_limit_rejects_before_the_next_provider_call() {
        let budget = LlmRunBudget::with_limits(2, u64::MAX);
        budget.reserve("a", "b").unwrap();
        budget.reserve("a", "b").unwrap();

        let error = budget.reserve("a", "b").unwrap_err();
        assert!(matches!(
            error,
            InfraError::LlmBudgetExceeded {
                resource: "provider_attempts",
                limit: 2,
                used: 2,
                requested: 1,
            }
        ));
    }

    #[test]
    fn in_flight_reservations_cannot_overbook_token_budget() {
        let one_request = request_token_ceiling("prompt", "system");
        let budget = LlmRunBudget::with_limits(10, one_request * 2 - 1);
        budget.reserve("prompt", "system").unwrap();

        let error = budget.reserve("prompt", "system").unwrap_err();
        assert!(matches!(
            error,
            InfraError::LlmBudgetExceeded {
                resource: "provider_tokens",
                ..
            }
        ));
    }

    #[test]
    fn exact_usage_replaces_the_conservative_reservation() {
        let one_request = request_token_ceiling("prompt", "system");
        let budget = LlmRunBudget::with_limits(10, one_request + 20);
        let reservation = budget.reserve("prompt", "system").unwrap();
        budget.settle(reservation, Some(&usage(10))).unwrap();

        budget.reserve("x", "y").unwrap();
    }

    #[test]
    fn failed_or_unreported_attempt_retains_its_full_reservation() {
        let one_request = request_token_ceiling("prompt", "system");
        let budget = LlmRunBudget::with_limits(10, one_request * 2 - 1);
        let reservation = budget.reserve("prompt", "system").unwrap();
        budget.settle(reservation, None).unwrap();

        assert!(matches!(
            budget.reserve("prompt", "system"),
            Err(InfraError::LlmBudgetExceeded {
                resource: "provider_tokens",
                ..
            })
        ));
    }

    #[test]
    fn request_ceiling_uses_utf8_bytes_and_fixed_provider_caps() {
        assert_eq!(
            request_token_ceiling("你", "好"),
            6 + PROVIDER_MAX_OUTPUT_TOKENS + CHAT_FRAMING_TOKEN_CEILING
        );
    }
}

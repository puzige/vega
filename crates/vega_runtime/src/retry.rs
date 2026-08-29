//! Retry policy for provider requests (tech-spec §4.1, S4-T19).
//!
//! Network errors and 5xx back off exponentially (1s / 2s / 4s, at most 3
//! retries); a 429 honors the server-provided `Retry-After` when present and
//! falls back to the exponential schedule otherwise. Retries only rebuild
//! the request — they never re-run tools or replay consumed events.

use std::time::Duration;

/// Retry schedule applied while establishing a provider stream.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Retries allowed after the initial attempt (default 3 → at most 4 attempts).
    pub max_retries: u32,
    /// Exponential base: attempt 0 sleeps 1×, attempt 1 sleeps 2×, attempt 2 sleeps 4×.
    pub base_delay: Duration,
    /// Upper bound for a single sleep (caps a large server-provided `Retry-After`).
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    /// 1s / 2s / 4s, at most 3 retries (tech-spec §4.1).
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Exponential backoff before the retry that follows `attempt` failed
    /// attempts (0-based): 1× / 2× / 4× … the base delay, capped at
    /// [`RetryPolicy::max_delay`].
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }

    /// Delay before the next attempt: the server-provided `Retry-After`
    /// (capped at [`RetryPolicy::max_delay`]) when present, the exponential
    /// schedule otherwise.
    pub fn delay_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        match retry_after {
            Some(delay) => delay.min(self.max_delay),
            None => self.backoff(attempt),
        }
    }
}

/// Parses a `Retry-After` header value in seconds form. The HTTP-date form
/// is deliberately not interpreted — callers fall back to the exponential
/// schedule instead of parsing calendar dates.
pub(crate) fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_backs_off_1s_2s_4s_with_three_retries() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.backoff(0), Duration::from_secs(1));
        assert_eq!(policy.backoff(1), Duration::from_secs(2));
        assert_eq!(policy.backoff(2), Duration::from_secs(4));
    }

    #[test]
    fn backoff_is_capped_at_max_delay() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.backoff(10), policy.max_delay);
    }

    #[test]
    fn delay_for_prefers_retry_after_and_caps_it() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.delay_for(0, Some(Duration::from_secs(2))),
            Duration::from_secs(2)
        );
        assert_eq!(
            policy.delay_for(0, Some(Duration::from_secs(3600))),
            policy.max_delay
        );
        assert_eq!(
            policy.delay_for(1, None),
            Duration::from_secs(2),
            "missing Retry-After falls back to the exponential schedule"
        );
    }

    #[test]
    fn parse_retry_after_accepts_seconds_only() {
        assert_eq!(parse_retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after(" 3 "), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
        assert_eq!(parse_retry_after("abc"), None);
        assert_eq!(parse_retry_after("-1"), None);
        // HTTP-date form intentionally unsupported.
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
    }
}

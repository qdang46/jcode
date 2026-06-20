//! Idle stream governor.
//!
//! Plan §7 reference to oh-my-pi:
//!   > Idle stream governor | first-event timeout + idle timeout
//!
//! When streaming a response from a provider, the runtime needs
//! to detect two failure modes:
//!
//!  - **First-event timeout**: the provider never sends the first
//!    byte. Indicates a network or auth failure.
//!  - **Idle timeout**: the provider sends some bytes, then stops
//!    responding for a while. Indicates a stalled stream.
//!
//! This module provides [`IdleStreamConfig`] (the two timeout
//! values) and a helper that returns the per-event deadline based
//! on whether the first event has been received yet.
//!
//! The actual stream wrapping is left to the consumer
//! (`jcode-llm-core`); this module is the policy side.

use std::time::Duration;

/// Idle stream policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleStreamConfig {
    /// How long to wait for the *first* event before declaring the
    /// stream dead. Should be larger than the typical time-to-
    /// first-byte (TTFB) for the provider.
    pub first_event_timeout: Duration,
    /// How long to wait between subsequent events before declaring
    /// the stream dead. Should be small enough to detect a stall
    /// but large enough to tolerate slow providers.
    pub idle_timeout: Duration,
}

impl Default for IdleStreamConfig {
    fn default() -> Self {
        Self {
            // 30 seconds: generous, but not so long that an
            // actually-dead stream goes unnoticed.
            first_event_timeout: Duration::from_secs(30),
            // 5 seconds: detects a stalled stream within a few
            // seconds.
            idle_timeout: Duration::from_secs(5),
        }
    }
}

/// Compute the deadline for the *next* event, based on whether the
/// first event has been seen yet.
pub fn next_deadline(
    config: &IdleStreamConfig,
    has_seen_first_event: bool,
    now: std::time::Instant,
) -> std::time::Instant {
    let timeout = if has_seen_first_event {
        config.idle_timeout
    } else {
        config.first_event_timeout
    };
    now + timeout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_sane() {
        let c = IdleStreamConfig::default();
        assert!(c.first_event_timeout > c.idle_timeout);
        assert!(c.first_event_timeout >= Duration::from_secs(5));
        assert!(c.idle_timeout >= Duration::from_millis(100));
    }

    #[test]
    fn first_event_deadline_uses_first_event_timeout() {
        let c = IdleStreamConfig::default();
        let now = std::time::Instant::now();
        let deadline = next_deadline(&c, false, now);
        let actual = deadline.duration_since(now);
        // Allow for a small clock skew.
        assert!(actual <= c.first_event_timeout);
        assert!(actual > c.first_event_timeout - Duration::from_millis(50));
    }

    #[test]
    fn subsequent_event_deadline_uses_idle_timeout() {
        let c = IdleStreamConfig::default();
        let now = std::time::Instant::now();
        let deadline = next_deadline(&c, true, now);
        let actual = deadline.duration_since(now);
        assert!(actual <= c.idle_timeout);
        assert!(actual > c.idle_timeout - Duration::from_millis(50));
    }

    #[test]
    fn custom_config_takes_precedence() {
        let c = IdleStreamConfig {
            first_event_timeout: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(1),
        };
        let now = std::time::Instant::now();
        let first = next_deadline(&c, false, now).duration_since(now);
        let idle = next_deadline(&c, true, now).duration_since(now);
        assert!(
            first > idle,
            "first-event timeout should be larger than idle"
        );
        assert!(first > Duration::from_secs(59));
        assert!(idle <= Duration::from_secs(1));
    }

    #[test]
    fn deadline_advances_with_now_for_same_flag() {
        // Calling next_deadline() twice with the same flag and
        // a later 'now' should produce a later deadline.
        let c = IdleStreamConfig::default();
        let now1 = std::time::Instant::now();
        let now2 = now1 + Duration::from_millis(10);
        let d1 = next_deadline(&c, false, now1);
        let d2 = next_deadline(&c, false, now2);
        assert!(d2 > d1, "deadline should advance with clock");
    }
}

//! Subscription usage-limit handling: decide, from a Claude `rate_limit_event`,
//! whether to auto-pause the agent and until when the limit window resets.
//!
//! Claude Code periodically emits a `rate_limit_event` stream-json line carrying
//! a `rate_limit_info` object: a `status` (`allowed` / `allowed_warning` /
//! `rejected`, plus the `overage_*` variants), the window `rateLimitType`, a
//! `resetsAt` unix timestamp, and a `utilization` percent that appears once the
//! window crosses Claude's early-warning threshold (~80%). We pause when a window
//! is rejected (exhausted) or its utilization has reached the operator's
//! configured threshold, and the gate resumes once the reset time passes.
//!
//! This module is the pure, unit-tested decision core; the orchestrator wires it
//! to the event stream and the settings row.

use serde_json::Value;

/// Given a `rate_limit_info` object and the operator's utilization `threshold`
/// (percent, 0-100), returns the unix reset timestamp to pause until, or `None`
/// if this notice does not warrant pausing.
pub fn pause_until(info: &Value, threshold: i32) -> Option<i64> {
    // The primary (non-overage) window.
    if let Some(reset) = window_pause(
        info.get("status").and_then(Value::as_str),
        info.get("utilization"),
        info.get("resetsAt").and_then(Value::as_i64),
        threshold,
    ) {
        return Some(reset);
    }
    // The pay-as-you-go overage window, if it's the one in use.
    if info
        .get("isUsingOverage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return window_pause(
            info.get("overageStatus").and_then(Value::as_str),
            info.get("overageUtilization"),
            info.get("overageResetsAt").and_then(Value::as_i64),
            threshold,
        );
    }
    None
}

/// Decides a single window: pause when it's rejected (exhausted) or its
/// early-warning has fired with utilization at/over the threshold.
fn window_pause(
    status: Option<&str>,
    utilization: Option<&Value>,
    resets_at: Option<i64>,
    threshold: i32,
) -> Option<i64> {
    match status {
        // Exhausted: the next call would be refused, so pause until reset.
        Some("rejected" | "overage_rejected") => resets_at,
        // Approaching: Claude reports `utilization` once the early-warning fires
        // (~80%). Pause if it has reached the operator's threshold; if the number
        // is absent, the warning itself is the signal.
        Some("allowed_warning") => {
            let over_threshold = utilization
                .and_then(parse_utilization)
                .is_none_or(|pct| pct >= f64::from(threshold));
            if over_threshold {
                resets_at
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Normalizes `utilization` to a 0-100 percent, accepting either a fraction
/// (`0.82`) or an already-scaled percent (`82`).
fn parse_utilization(value: &Value) -> Option<f64> {
    let raw = value.as_f64()?;
    Some(if raw <= 1.0 { raw * 100.0 } else { raw })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allowed_window_never_pauses() {
        let info = json!({ "status": "allowed", "rateLimitType": "five_hour", "resetsAt": 1000 });
        assert_eq!(pause_until(&info, 80), None);
    }

    #[test]
    fn rejected_pauses_until_reset_regardless_of_threshold() {
        let info = json!({ "status": "rejected", "resetsAt": 1781142000_i64 });
        assert_eq!(pause_until(&info, 95), Some(1781142000));
    }

    #[test]
    fn warning_pauses_once_utilization_reaches_threshold() {
        // Percent form.
        let info = json!({ "status": "allowed_warning", "utilization": 82, "resetsAt": 500 });
        assert_eq!(pause_until(&info, 80), Some(500));
        // Fraction form normalizes to the same percent.
        let frac = json!({ "status": "allowed_warning", "utilization": 0.82, "resetsAt": 500 });
        assert_eq!(pause_until(&frac, 80), Some(500));
    }

    #[test]
    fn warning_below_threshold_does_not_pause() {
        let info = json!({ "status": "allowed_warning", "utilization": 70, "resetsAt": 500 });
        assert_eq!(pause_until(&info, 80), None);
    }

    #[test]
    fn warning_without_utilization_pauses_on_the_signal_alone() {
        let info = json!({ "status": "allowed_warning", "resetsAt": 500 });
        assert_eq!(pause_until(&info, 80), Some(500));
    }

    #[test]
    fn overage_rejected_pauses_until_overage_reset() {
        let info = json!({
            "status": "allowed",
            "resetsAt": 1000,
            "isUsingOverage": true,
            "overageStatus": "overage_rejected",
            "overageResetsAt": 2000
        });
        assert_eq!(pause_until(&info, 80), Some(2000));
    }
}

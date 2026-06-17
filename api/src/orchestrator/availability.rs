//! The optional availability schedule that gates when the agent picks up work.
//!
//! Operators can restrict the agent to specific hours and weekdays in their own
//! time zone, and skip individual dates (vacations, holidays). The check is a
//! single pure function, [`is_available`], so the clock is injected and the
//! whole policy is unit-testable without touching the system time.

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use tracing::warn;

use crate::db::models::Settings;

/// Minutes in a day, the exclusive upper bound for a window's minute-of-day.
const MINUTES_PER_DAY: u32 = 24 * 60;

/// Whether the agent may pick up new work at `now`, given the schedule settings.
///
/// The schedule is entirely optional. When [`Settings::availability_enabled`] is
/// false the agent always runs. When enabled, `now` is converted into the
/// configured IANA time zone and then evaluated in two steps:
///
/// 1. If the local date is in [`Settings::availability_skip_dates`], the whole
///    day is off.
/// 2. Otherwise the local time of day must fall inside one of the weekly
///    [`Settings::availability_windows`]. An empty window list means "any time of
///    day", so an operator can pause for specific dates without also having to
///    define working hours.
///
/// An unparseable time zone fails open (the agent runs) and logs a warning, so a
/// configuration typo never silently halts all work.
pub fn is_available(settings: &Settings, now: DateTime<Utc>) -> bool {
    if !settings.availability_enabled {
        return true;
    }

    let Ok(timezone) = settings.availability_timezone.parse::<Tz>() else {
        warn!(
            timezone = %settings.availability_timezone,
            "invalid availability time zone; ignoring the schedule"
        );
        return true;
    };

    let local = now.with_timezone(&timezone);

    // A skipped date (holiday or vacation) blocks the entire day.
    if settings
        .availability_skip_dates
        .contains(&local.date_naive())
    {
        return false;
    }

    let windows = &settings.availability_windows.0;
    if windows.is_empty() {
        return true;
    }

    let weekday = u8::try_from(local.weekday().num_days_from_monday())
        .expect("weekday index is always 0..=6");
    let minute_of_day = u16::try_from(local.hour() * 60 + local.minute())
        .expect("minute of day is always 0..MINUTES_PER_DAY");
    debug_assert!(u32::from(minute_of_day) < MINUTES_PER_DAY);

    windows.iter().any(|window| {
        window.weekday == weekday
            && minute_of_day >= window.start_minute
            && minute_of_day < window.end_minute
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{AvailabilityWindow, NetworkAccessLevel, ReviewPolicy};
    use chrono::TimeZone;
    use sqlx::types::Json;

    /// A settings row with the schedule fields set and everything else inert.
    fn settings_with_schedule(
        enabled: bool,
        timezone: &str,
        windows: Vec<AvailabilityWindow>,
        skip_dates: Vec<chrono::NaiveDate>,
    ) -> Settings {
        Settings {
            org_name: String::new(),
            global_instructions: String::new(),
            default_review_policy: ReviewPolicy::None,
            agent_paused: false,
            claude_model: String::new(),
            workspace_image_tag: String::new(),
            base_setup_script: String::new(),
            config_repo_url: String::new(),
            default_branch_template: String::new(),
            config_repo_error: None,
            current_session_id: None,
            updated_at: Utc::now(),
            claude_token_set: false,
            claude_auth_mode: crate::db::models::ClaudeAuthMode::Subscription,
            claude_account_email: String::new(),
            claude_usage_token_set: false,
            github_token_set: false,
            availability_enabled: enabled,
            availability_timezone: timezone.to_string(),
            availability_windows: Json(windows),
            availability_skip_dates: Json(skip_dates),
            network_access_level: NetworkAccessLevel::Full,
            network_access_domains: Json(Vec::new()),
            network_access_include_defaults: true,
            usage_limit_pause_enabled: true,
            usage_limit_threshold: 80,
            usage_paused_until: None,
            railway_idle_timeout_minutes: 30,
            post_thoughts_enabled: false,
            close_issue_on_done: true,
            jira_enabled: false,
            jira_deployment: crate::db::models::JiraDeployment::Cloud,
            jira_base_url: String::new(),
            jira_email: String::new(),
            jira_assigned_to_me_only: true,
            jira_account_id: String::new(),
            jira_token_set: false,
            github_webhook_secret_set: false,
            jira_webhook_secret_set: false,
            attention_sound_enabled: true,
            completion_sound_enabled: true,
            attention_sound_custom: false,
            completion_sound_custom: false,
            jira_token_preview: None,
            claude_token_preview: None,
            github_token_preview: None,
            cooldown_until: None,
        }
    }

    /// Monday through Friday, 9:00 to 17:00.
    fn business_hours() -> Vec<AvailabilityWindow> {
        (0..5)
            .map(|weekday| AvailabilityWindow {
                weekday,
                start_minute: 9 * 60,
                end_minute: 17 * 60,
            })
            .collect()
    }

    #[test]
    fn disabled_schedule_always_runs() {
        let settings = settings_with_schedule(false, "America/Denver", business_hours(), vec![]);
        // 3am on a Sunday: well outside business hours, but the schedule is off.
        let now = Utc.with_ymd_and_hms(2026, 6, 7, 9, 0, 0).unwrap();
        assert!(is_available(&settings, now));
    }

    #[test]
    fn inside_window_runs_and_outside_does_not() {
        let settings = settings_with_schedule(true, "America/Denver", business_hours(), vec![]);

        // Mountain Daylight Time is UTC-6 in June, so 16:00Z is 10:00 local on a
        // Wednesday: inside the 9-5 window.
        let inside = Utc.with_ymd_and_hms(2026, 6, 10, 16, 0, 0).unwrap();
        assert!(is_available(&settings, inside));

        // 02:00Z the same day is 20:00 the previous (Tuesday) evening: outside.
        let outside = Utc.with_ymd_and_hms(2026, 6, 10, 2, 0, 0).unwrap();
        assert!(!is_available(&settings, outside));
    }

    #[test]
    fn weekend_is_excluded() {
        let settings = settings_with_schedule(true, "America/Denver", business_hours(), vec![]);
        // Saturday 14:00 local (20:00Z): a weekday-only schedule excludes it.
        let saturday = Utc.with_ymd_and_hms(2026, 6, 13, 20, 0, 0).unwrap();
        assert!(!is_available(&settings, saturday));
    }

    #[test]
    fn skip_date_blocks_the_whole_day() {
        let independence_day = chrono::NaiveDate::from_ymd_opt(2026, 7, 3).unwrap();
        let settings = settings_with_schedule(
            true,
            "America/Denver",
            business_hours(),
            vec![independence_day],
        );
        // Friday 2026-07-03, 16:00Z = 10:00 local: inside hours but a skip date.
        let now = Utc.with_ymd_and_hms(2026, 7, 3, 16, 0, 0).unwrap();
        assert!(!is_available(&settings, now));
    }

    #[test]
    fn empty_windows_mean_any_time_subject_to_skip_dates() {
        let vacation = chrono::NaiveDate::from_ymd_opt(2026, 6, 14).unwrap();
        let settings = settings_with_schedule(true, "America/Denver", vec![], vec![vacation]);

        // A random Sunday 03:00 local is fine with no windows defined.
        let normal_day = Utc.with_ymd_and_hms(2026, 6, 7, 9, 0, 0).unwrap();
        assert!(is_available(&settings, normal_day));

        // But the explicit skip date is still off.
        let skipped = Utc.with_ymd_and_hms(2026, 6, 14, 18, 0, 0).unwrap();
        assert!(!is_available(&settings, skipped));
    }

    #[test]
    fn invalid_timezone_fails_open() {
        let settings = settings_with_schedule(true, "Not/AZone", business_hours(), vec![]);
        let now = Utc.with_ymd_and_hms(2026, 6, 13, 20, 0, 0).unwrap();
        // Would be excluded (Saturday) if the zone parsed; instead it runs.
        assert!(is_available(&settings, now));
    }
}

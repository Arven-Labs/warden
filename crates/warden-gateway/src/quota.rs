//! Minimal spend caps, checked when a call reserves its hold.
//!
//! An account may carry a daily cap (the current UTC day) and a monthly one (a rolling 30 days), in micro-USD. A call is
//! refused when what the account was already charged in the window plus the call's hold would pass the cap. The
//! answer ceiling (`max_tokens`) lives in the catalog, per model. Not here: rate limiting by token or IP (`ratelimit`) and the
//! suspension of an abnormal spike, which is the hourly cap in `Store::settle` (HOSPEDAGEM.md C4).

use crate::store::{Account, Micros};

pub const HOUR_SECS: i64 = 3_600;
pub const DAY_SECS: i64 = 86_400;
pub const MONTH_SECS: i64 = 30 * DAY_SECS;

/// Which cap a call ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Daily,
    Monthly,
}

impl Cap {
    pub fn name(self) -> &'static str {
        match self {
            Cap::Daily => "daily",
            Cap::Monthly => "monthly",
        }
    }
}

/// The start of the current UTC day.
pub fn day_start(now: i64) -> i64 {
    now - now.rem_euclid(DAY_SECS)
}

/// The start of the rolling month window.
pub fn month_start(now: i64) -> i64 {
    now - MONTH_SECS
}

/// `Ok` if the call fits under both caps. `spent_today` and `spent_month` are what the account was already charged in each window.
pub fn check(account: &Account, spent_today: Micros, spent_month: Micros, hold: Micros) -> Result<(), Cap> {
    if account.daily_cap.is_some_and(|cap| spent_today + hold > cap) {
        return Err(Cap::Daily);
    }
    if account.monthly_cap.is_some_and(|cap| spent_month + hold > cap) {
        return Err(Cap::Monthly);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(daily: Option<Micros>, monthly: Option<Micros>) -> Account {
        Account { id: "a".into(), name: "a".into(), active: true, daily_cap: daily, monthly_cap: monthly, created_at: 0 }
    }

    #[test]
    fn no_caps_means_no_limit() {
        assert_eq!(check(&account(None, None), i64::MAX / 4, i64::MAX / 4, 1), Ok(()));
    }

    #[test]
    fn a_call_that_lands_exactly_on_the_cap_fits() {
        assert_eq!(check(&account(Some(1_000), None), 900, 900, 100), Ok(()));
        assert_eq!(check(&account(Some(1_000), None), 900, 900, 101), Err(Cap::Daily));
    }

    #[test]
    fn each_cap_is_checked_on_its_own_window() {
        let a = account(Some(1_000), Some(5_000));
        assert_eq!(check(&a, 0, 4_950, 100), Err(Cap::Monthly));
        assert_eq!(check(&a, 950, 950, 100), Err(Cap::Daily));
        assert_eq!(check(&a, 100, 4_000, 100), Ok(()));
    }

    #[test]
    fn the_windows_start_where_they_should() {
        let noon = 1_800_000_000 - 1_800_000_000 % DAY_SECS + 12 * 3600;
        assert_eq!(day_start(noon), noon - 12 * 3600);
        assert_eq!(month_start(noon), noon - 30 * 86_400);
        assert_eq!(day_start(0), 0);
    }
}

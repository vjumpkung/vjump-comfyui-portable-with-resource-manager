use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use application::shared::{Clock, IdGenerator};

/// Formats a Unix timestamp (seconds) as RFC 3339 UTC.
pub fn rfc3339_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        remainder / 3_600,
        remainder % 3_600 / 60,
        remainder % 60
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        // Millisecond precision keeps records created within one second ordered.
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let whole = rfc3339_utc(elapsed.as_secs());
        format!(
            "{}.{:03}Z",
            &whole[..whole.len() - 1],
            elapsed.subsec_millis()
        )
    }
}

/// Unique, non-secret identifiers: 128 random bits from the standard library's per-process
/// random keys, mixed with a counter and the clock.
#[derive(Debug, Default)]
pub struct RandomIds {
    counter: AtomicU64,
}

impl IdGenerator for RandomIds {
    fn new_id(&self) -> String {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64);
        let mut high = RandomState::new().build_hasher();
        high.write_u64(count);
        high.write_u64(nanos);
        let mut low = RandomState::new().build_hasher();
        low.write_u64(nanos);
        low.write_u64(count);
        format!("{:016x}{:016x}", high.finish(), low.finish())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn timestamps_match_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_792_000_000), "2026-10-14T17:46:40Z");
        assert_eq!(rfc3339_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn the_system_clock_is_rfc3339_with_milliseconds_and_ordered() {
        let first = SystemClock.now();
        let second = SystemClock.now();
        assert_eq!(first.len(), "2026-10-16T01:02:03.123Z".len(), "{first}");
        assert!(first.ends_with('Z') && first.as_bytes()[19] == b'.');
        assert!(first <= second);
    }

    #[test]
    fn ids_are_unique_lowercase_hex() {
        let ids = RandomIds::default();
        let all: HashSet<String> = (0..1_000).map(|_| ids.new_id()).collect();
        assert_eq!(all.len(), 1_000);
        assert!(all.iter().all(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }));
    }
}

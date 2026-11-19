//! Calendar dates for the replay screen (FileTimeToLocalFileTime and
//! FileTimeToSystemTime). The platform supplies the local UTC offset.

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UTC_OFFSET: AtomicI32 = AtomicI32::new(0);

/// Seconds east of UTC of the local time zone.
pub fn set_utc_offset(seconds: i32) {
    UTC_OFFSET.store(seconds, Ordering::Relaxed);
}

/// (year, month, day, hour, minute) of `t` in local time.
pub fn local(t: SystemTime) -> (i64, u32, u32, u32, u32) {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    } + UTC_OFFSET.load(Ordering::Relaxed) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32)
}

/// "%04d.%02d.%02d %02d:%02d".
pub fn format_local(t: SystemTime) -> String {
    let (y, mo, d, h, mi) = local(t);
    format!("{y:04}.{mo:02}.{d:02} {h:02}:{mi:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates() {
        set_utc_offset(0);
        assert_eq!(format_local(UNIX_EPOCH + Duration::from_secs(1_700_000_000)), "2023.11.14 22:13");
        assert_eq!(format_local(UNIX_EPOCH), "1970.01.01 00:00");
    }
}

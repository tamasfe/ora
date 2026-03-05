use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn systemtime_from_ts(ts: f64) -> SystemTime {
    Duration::try_from_secs_f64(ts)
        .map(|d| UNIX_EPOCH + d)
        .unwrap_or(UNIX_EPOCH)
}

pub(crate) fn systemtime_to_ts(st: SystemTime) -> f64 {
    st.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

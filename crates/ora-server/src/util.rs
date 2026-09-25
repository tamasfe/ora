use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ora_backend::common::Label;
use serde::de::IgnoredAny;

pub(crate) fn validate_json(json_str: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<IgnoredAny>(json_str).map(|_| ())
}

pub(crate) fn deduplicate_labels(labels: &mut Vec<Label>) {
    let mut seen = std::collections::HashSet::new();
    labels.retain(|l| seen.insert(l.key.clone()));
}

pub(crate) fn inherit_labels(parent: &[Label], child: &mut Vec<Label>) {
    let mut existing_keys: std::collections::HashSet<String> =
        child.iter().map(|l| l.key.clone()).collect();

    for label in parent {
        if !existing_keys.contains(&label.key) {
            child.push(label.clone());
            existing_keys.insert(label.key.clone());
        }
    }
}

/// The instant after the given duration from now.
///
/// Durations that are not representable (e.g. [`Duration::MAX`]
/// used as "forever") result in an instant far in the future.
pub(crate) fn deadline_after(duration: std::time::Duration) -> std::time::Instant {
    let now = std::time::Instant::now();

    now.checked_add(duration)
        .or_else(|| now.checked_add(std::time::Duration::from_hours(30 * 365 * 24)))
        .unwrap_or(now)
}

/// The earliest supported time, `0001-01-01T00:00:00Z`.
const MIN_SECONDS_BEFORE_EPOCH: u64 = 62_135_596_800;

/// The first unsupported time after the supported range, `10000-01-01T00:00:00Z`.
const MAX_SECONDS_AFTER_EPOCH: u64 = 253_402_300_800;

/// Validate that a time is in the range of `google.protobuf.Timestamp`
/// (from the year 1 to 9999), times outside of it might not be supported
/// by backends.
pub(crate) fn validate_time(time: SystemTime) -> Result<(), &'static str> {
    let supported = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after < Duration::from_secs(MAX_SECONDS_AFTER_EPOCH),
        Err(before) => before.duration() <= Duration::from_secs(MIN_SECONDS_BEFORE_EPOCH),
    };

    if supported {
        Ok(())
    } else {
        Err("time must be between the years 1 and 9999")
    }
}

/// Validate that labels contain no NUL characters,
/// which are not supported by all backends.
pub(crate) fn validate_labels(labels: &[Label]) -> Result<(), String> {
    for label in labels {
        if label.key.contains('\0') || label.value.contains('\0') {
            return Err(format!(
                "label '{}' must not contain NUL characters",
                label.key.escape_default()
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_range() {
        let max = UNIX_EPOCH + Duration::from_secs(MAX_SECONDS_AFTER_EPOCH);
        assert!(validate_time(UNIX_EPOCH).is_ok());
        assert!(validate_time(max - Duration::from_nanos(1)).is_ok());
        assert!(validate_time(max).is_err());

        if let Some(min) = UNIX_EPOCH.checked_sub(Duration::from_secs(MIN_SECONDS_BEFORE_EPOCH)) {
            assert!(validate_time(min).is_ok());
            assert!(validate_time(min - Duration::from_nanos(1)).is_err());
        }
    }

    #[test]
    fn labels_without_nul() {
        let label = |key: &str, value: &str| Label {
            key: key.to_string(),
            value: value.to_string(),
        };

        assert!(validate_labels(&[label("a", "b"), label("", "")]).is_ok());
        assert!(validate_labels(&[label("a\0", "b")]).is_err());
        assert!(validate_labels(&[label("a", "b\0")]).is_err());
    }
}

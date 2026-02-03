//! Job type definitions.

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// A type that is used as a placeholder when a job type is unknown,
/// e.g. when listing jobs or schedules.
pub enum AnyJobType {}

/// A job type.
///
/// Types implementing this trait are also implicitly
/// the input types for jobs of this type.
pub trait JobType: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static {
    /// The output type of the job.
    type Output: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static;

    /// Return the ID of this job type.
    fn job_type_id() -> JobTypeId;
}

/// A Case-sensitive identifier for a job type.
///
/// The ID must conform to the following rules:
///
/// - An ID is made up of segments separated by dots (`.`).
/// - Each segment must start with a letter (A-Z, a-z) and can be followed by
///   letters, digits (0-9), or underscores (`_`).
/// - The ID must not start or end with a dot, and must not contain consecutive dots.
///
/// Examples of valid IDs:
/// - `data_processing`
/// - `image.resize_v2`
/// - `myService.analytics.GenerateReport`
///
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
#[must_use]
pub struct JobTypeId(Cow<'static, str>);

impl std::fmt::Display for JobTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl JobTypeId {
    /// Create a new job type ID.
    pub fn new<S: Into<Cow<'static, str>>>(s: S) -> Result<Self, InvalidJobTypeId> {
        let s = s.into();

        if !Self::validate(&s) {
            return Err(InvalidJobTypeId(s.to_string()));
        }

        Ok(Self(s))
    }

    /// Create a new job type ID from a static string.
    /// Should be used in const contexts.
    ///
    /// # Panics
    ///
    /// Panics if the given string is not a valid job type ID.
    pub const fn new_const(s: &'static str) -> JobTypeId {
        assert!(
            Self::validate(s),
            "the given string is not a valid job type ID"
        );

        Self(Cow::Borrowed(s))
    }

    /// Return whether the given string is a valid job type ID.
    #[must_use]
    pub const fn validate(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        let bytes = s.as_bytes();
        let mut i = 0;
        let len = bytes.len();
        let mut segment_start = true;

        while i < len {
            let c = bytes[i];

            if c == b'.' {
                if segment_start {
                    return false;
                }
                segment_start = true;
            } else if segment_start {
                if !(c >= b'A' && c <= b'Z' || c >= b'a' && c <= b'z') {
                    return false;
                }
                segment_start = false;
            } else if !(c.is_ascii_alphanumeric() || c == b'_') {
                return false;
            }

            i += 1;
        }

        if segment_start {
            return false;
        }

        true
    }

    /// Get the job type ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the inner string.
    #[must_use]
    pub fn into_inner(self) -> Cow<'static, str> {
        self.0
    }
}

/// An error indicating that a job type ID is invalid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidJobTypeId(pub String);

impl std::fmt::Display for InvalidJobTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"invalid job type ID: "{}""#, self.0)
    }
}

impl core::error::Error for InvalidJobTypeId {}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_job_type_id_validation() {
        let valid_ids = [
            "data_processing",
            "image.resize_v2",
            "myService.analytics.GenerateReport",
            "A",
            "a.b_c.d1",
        ];

        let invalid_ids = [
            "",
            ".startsWithDot",
            "endsWithDot.",
            "consecutive..dots",
            "invalid-char$",
            "1startsWithDigit",
            "segment-with-hyphen",
        ];

        for id in &valid_ids {
            assert!(JobTypeId::validate(id), "expected '{id}' to be valid");
        }

        for id in &invalid_ids {
            assert!(!JobTypeId::validate(id), "expected '{id}' to be invalid");
        }
    }
}

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

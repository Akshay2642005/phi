//! Immutable metadata for a workflow definition.
//!
//! [`WorkflowMetadata`] holds optional descriptive and organisational
//! information about a workflow.  It is designed to be extended with
//! additional fields over time without breaking existing callers — the
//! struct uses `#[non_exhaustive]` and all fields are accessed through
//! public getter methods.
//!
//! # Ownership
//! Getter methods return references (`&str`, `&[String]`, `&HashMap`) to
//! avoid cloning.  The struct is `Clone` if an owned copy is needed.
//!
//! # Extensibility
//! New fields can be added to `WorkflowMetadata` without a breaking change
//! because:
//! - The struct is `#[non_exhaustive]` — external code cannot name or
//!   construct it directly.
//! - Internal construction goes through `new()` or via the
//!   [`WorkflowBuilder`](crate::core::workflow::builder::WorkflowBuilder).
//! - Getters are added alongside new fields; existing callers continue
//!   to compile without changes.

use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WorkflowMetadata {
    created_at: Option<SystemTime>,
    author: Option<String>,
    tags: Vec<String>,
    labels: HashMap<String, String>,
    custom: HashMap<String, String>,
}

impl WorkflowMetadata {
    #[inline]
    pub fn new() -> Self {
        Self {
            created_at: None,
            author: None,
            tags: Vec::new(),
            labels: HashMap::new(),
            custom: HashMap::new(),
        }
    }

    #[inline]
    pub fn created_at(&self) -> Option<SystemTime> {
        self.created_at
    }

    #[inline]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    #[inline]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    #[inline]
    pub fn labels(&self) -> &HashMap<String, String> {
        &self.labels
    }

    #[inline]
    pub fn custom(&self) -> &HashMap<String, String> {
        &self.custom
    }

    pub fn iter_labels(&self) -> impl Iterator<Item = (&str, &str)> {
        self.labels.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn iter_custom(&self) -> impl Iterator<Item = (&str, &str)> {
        self.custom.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    #[inline]
    pub(crate) fn set_created_at(&mut self, ts: SystemTime) {
        self.created_at = Some(ts);
    }

    #[inline]
    pub(crate) fn set_author(&mut self, author: String) {
        self.author = Some(author);
    }

    #[inline]
    pub(crate) fn add_tag(&mut self, tag: String) {
        self.tags.push(tag);
    }

    #[inline]
    pub(crate) fn insert_label(&mut self, key: String, value: String) {
        self.labels.insert(key, value);
    }

    #[inline]
    pub(crate) fn insert_custom(&mut self, key: String, value: String) {
        self.custom.insert(key, value);
    }
}

impl Default for WorkflowMetadata {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_metadata_is_empty() {
        let m = WorkflowMetadata::new();
        assert!(m.created_at().is_none());
        assert!(m.author().is_none());
        assert!(m.tags().is_empty());
        assert!(m.labels().is_empty());
        assert!(m.custom().is_empty());
    }

    #[test]
    fn builder_sets_created_at() {
        let mut m = WorkflowMetadata::new();
        let ts = SystemTime::now();
        m.set_created_at(ts);
        assert_eq!(m.created_at(), Some(ts));
    }

    #[test]
    fn builder_sets_author() {
        let mut m = WorkflowMetadata::new();
        m.set_author("Alice".to_string());
        assert_eq!(m.author(), Some("Alice"));
    }

    #[test]
    fn builder_adds_tags() {
        let mut m = WorkflowMetadata::new();
        m.add_tag("critical".to_string());
        m.add_tag("prod".to_string());
        assert_eq!(m.tags().len(), 2);
        assert_eq!(m.tags()[0], "critical");
        assert_eq!(m.tags()[1], "prod");
    }

    #[test]
    fn builder_inserts_labels() {
        let mut m = WorkflowMetadata::new();
        m.insert_label("env".to_string(), "prod".to_string());
        m.insert_label("team".to_string(), "platform".to_string());
        assert_eq!(m.labels().len(), 2);
        assert_eq!(m.labels().get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn builder_overwrites_label() {
        let mut m = WorkflowMetadata::new();
        m.insert_label("env".to_string(), "staging".to_string());
        m.insert_label("env".to_string(), "prod".to_string());
        assert_eq!(m.labels().len(), 1);
        assert_eq!(m.labels().get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn builder_inserts_custom() {
        let mut m = WorkflowMetadata::new();
        m.insert_custom("source".to_string(), "migration-v2".to_string());
        assert_eq!(
            m.custom().get("source").map(String::as_str),
            Some("migration-v2")
        );
    }

    #[test]
    fn iter_labels_yields_all_entries() {
        let mut m = WorkflowMetadata::new();
        m.insert_label("a".to_string(), "1".to_string());
        m.insert_label("b".to_string(), "2".to_string());
        let pairs: Vec<(&str, &str)> = m.iter_labels().collect();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.contains(&("a", "1")));
        assert!(pairs.contains(&("b", "2")));
    }

    #[test]
    fn iter_custom_yields_all_entries() {
        let mut m = WorkflowMetadata::new();
        m.insert_custom("key".to_string(), "val".to_string());
        let pairs: Vec<(&str, &str)> = m.iter_custom().collect();
        assert_eq!(pairs, vec![("key", "val")]);
    }

    #[test]
    fn default_is_same_as_new() {
        assert_eq!(
            WorkflowMetadata::default().created_at(),
            WorkflowMetadata::new().created_at()
        );
    }

    #[test]
    fn clone_produces_independent_copy() {
        let mut m = WorkflowMetadata::new();
        m.set_author("Bob".to_string());
        let mut cloned = m.clone();
        cloned.set_author("Charlie".to_string());
        assert_eq!(m.author(), Some("Bob"));
        assert_eq!(cloned.author(), Some("Charlie"));
    }
}

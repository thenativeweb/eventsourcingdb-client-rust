use serde::Serialize;

/// Enum for different preconditions that can be used when writing events
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum Precondition {
    /// Check if the subject with the given path has no other events
    #[serde(rename = "isSubjectPristine")]
    IsSubjectPristine {
        /// The subject to check
        subject: String,
    },
    /// Check if the subject with the given path has no other events
    #[serde(rename = "isSubjectOnEventId")]
    IsSubjectOnEventId {
        /// The subject to check
        subject: String,
        /// The event ID to check against
        #[serde(rename = "eventId")]
        event_id: String,
    },
    /// Check if an EventQL query returns true
    #[serde(rename = "isEventQlTrue")]
    IsEventQLTrue {
        /// The EventQL query to check
        query: String,
    },
}

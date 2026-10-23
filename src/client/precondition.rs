use serde::Serialize;

/// Options for a precondition on a subject, which [`Precondition::is_subject_pristine`],
/// [`Precondition::is_subject_populated`], and [`Precondition::is_subject_on_event_id`] take
///
/// ```
/// use eventsourcingdb::{Precondition, PreconditionOptions};
///
/// let precondition = Precondition::is_subject_pristine(
///     "/orders/42",
///     Some(PreconditionOptions {
///         recursive: true,
///         ..Default::default()
///     }),
/// );
/// ```
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreconditionOptions {
    /// Whether the precondition applies to the subject together with all subjects below it, or to the subject alone,
    /// which is the default
    ///
    /// Only the v2 API checks a precondition recursively, so if the server does not offer it, writing the events fails
    /// with [`ClientError::IncompatibleServer`](crate::error::ClientError::IncompatibleServer) before the events are
    /// sent.
    pub recursive: bool,
}

/// Enum for different preconditions that can be used when writing events
///
/// Create a precondition with its function, such as [`Precondition::is_subject_pristine`]. The variants are
/// `#[non_exhaustive]`, so that a new field does not break the code that creates or matches them.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum Precondition {
    /// Check if the subject with the given path has no events, see [`Precondition::is_subject_pristine`]
    #[serde(rename = "isSubjectPristine")]
    #[non_exhaustive]
    IsSubjectPristine {
        /// The subject to check
        subject: String,
        /// The options of the precondition, which are left out of the request if there are none
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<PreconditionOptions>,
    },
    /// Check if the subject with the given path already has events, see [`Precondition::is_subject_populated`]
    #[serde(rename = "isSubjectPopulated")]
    #[non_exhaustive]
    IsSubjectPopulated {
        /// The subject to check
        subject: String,
        /// The options of the precondition, which are left out of the request if there are none
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<PreconditionOptions>,
    },
    /// Check if the latest event of the subject with the given path has the given ID, see
    /// [`Precondition::is_subject_on_event_id`]
    #[serde(rename = "isSubjectOnEventId")]
    #[non_exhaustive]
    IsSubjectOnEventId {
        /// The subject to check
        subject: String,
        /// The event ID to check against
        #[serde(rename = "eventId")]
        event_id: String,
        /// The options of the precondition, which are left out of the request if there are none
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<PreconditionOptions>,
    },
    /// Check if an EventQL query returns true, see [`Precondition::is_eventql_query_true`]
    #[serde(rename = "isEventQlQueryTrue")]
    #[non_exhaustive]
    IsEventQLQueryTrue {
        /// The EventQL query to check
        query: String,
    },
}

impl Precondition {
    /// Creates a precondition that holds if the subject has no events.
    ///
    /// With the option `recursive`, it holds if neither the subject nor any subject below it has events, which needs
    /// a server that offers the v2 API.
    #[must_use]
    pub fn is_subject_pristine(
        subject: impl Into<String>,
        options: Option<PreconditionOptions>,
    ) -> Self {
        Self::IsSubjectPristine {
            subject: subject.into(),
            options,
        }
    }

    /// Creates a precondition that holds if the subject has at least one event.
    ///
    /// With the option `recursive`, it holds if the subject or any subject below it has an event, which needs a server
    /// that offers the v2 API.
    #[must_use]
    pub fn is_subject_populated(
        subject: impl Into<String>,
        options: Option<PreconditionOptions>,
    ) -> Self {
        Self::IsSubjectPopulated {
            subject: subject.into(),
            options,
        }
    }

    /// Creates a precondition that holds if the latest event of the subject has the given ID.
    ///
    /// With the option `recursive`, it holds if the latest event among the subject and all subjects below it has the
    /// given ID, which needs a server that offers the v2 API.
    #[must_use]
    pub fn is_subject_on_event_id(
        subject: impl Into<String>,
        event_id: impl Into<String>,
        options: Option<PreconditionOptions>,
    ) -> Self {
        Self::IsSubjectOnEventId {
            subject: subject.into(),
            event_id: event_id.into(),
            options,
        }
    }

    /// Creates a precondition that holds if the EventQL query returns `true`.
    #[must_use]
    pub fn is_eventql_query_true(query: impl Into<String>) -> Self {
        Self::IsEventQLQueryTrue {
            query: query.into(),
        }
    }
}

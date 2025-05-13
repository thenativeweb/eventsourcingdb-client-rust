//! This module holds all event types that are send between the client and the database.

// Allow module inception here, since "event" is the expected as the name for both modules.
// Renaming would be possible, but would probably lead to more confusion.
#[allow(clippy::module_inception)]
mod event;
mod event_candidate;
mod management_event;

pub use event::Event;
pub use event_candidate::EventCandidate;
pub use management_event::ManagementEvent;
use serde::{Deserialize, Serialize};

#[cfg(feature="cloudevents")]
use crate::error::EventError;

/// Represents the trace information of an event.
/// This is used for distributed tracing.
/// It can either be a traceparent or a traceparent and tracestate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TraceInfo {
    // LEAVE ORDER AS IS
    // This is important for deserialization as the traceparent is always present

    /// The traceparent and tracestate of the event.
    /// This is used for distributed tracing.
    WithState {
        /// The traceparent of the event.
        /// This is used for distributed tracing.
        traceparent: String,
        /// The tracestate of the event.
        /// This is used for distributed tracing.
        tracestate: String,
    },
    /// The traceparent of the event.
    /// This is used for distributed tracing.
    Traceparent {
        /// The traceparent of the event.
        /// This is used for distributed tracing.
        traceparent: String,
    },
}

impl TraceInfo {
    /// Get the traceparent of the event.
    #[must_use]
    pub fn traceparent(&self) -> &str {
        match self {
            Self::Traceparent { traceparent } | Self::WithState { traceparent, .. } => traceparent,
        }
    }
    /// Get the tracestate of the event.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        match self {
            Self::Traceparent { .. } => None,
            Self::WithState { tracestate, .. } => Some(tracestate),
        }
    }

    /// Create a new `TraceInfo` from a cloudevent.
    /// This will return None if the cloudevent does not contain a traceparent or tracestate.
    ///
    /// # Errors
    /// If the cloudevent contains a tracestate but no traceparent, an error will be returned.
    #[cfg(feature="cloudevents")]
    pub fn from_cloudevent(event: &cloudevents::Event) -> Result<Option<Self>, EventError> {
        let traceparent = event.extension("traceparent").map(ToString::to_string);
        let tracestate = event.extension("tracestate").map(ToString::to_string);

        match (traceparent, tracestate) {
            (Some(traceparent), Some(tracestate)) => Ok(Some(Self::WithState {
                traceparent,
                tracestate,
            })),
            (Some(traceparent), None) => Ok(Some(Self::Traceparent { traceparent })),
            (None, None) => Ok(None),
            (None, Some(_)) => Err(EventError::InvalidCloudevent),
        }
    }
}

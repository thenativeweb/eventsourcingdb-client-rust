//! This module holds supporting traits for the "Tracing" feature of eventsourcingdb.

use serde::{Deserialize, Serialize};

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
    #[cfg(feature = "cloudevents")]
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

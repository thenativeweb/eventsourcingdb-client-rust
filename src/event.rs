//! This module holds all event types that are send between the client and the database.

mod event_types;
mod trace_info;

// Reexport relevant types to flatten the module graph for consumers and
// keep private encapsulation of implementation details.
pub use event_types::event::Event;
pub use event_types::event_candidate::EventCandidate;
pub use event_types::management_event::ManagementEvent;
pub use trace_info::TraceInfo;

#[cfg(feature = "cloudevents")]
pub use crate::error::EventError;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(feature = "cloudevents")]
use cloudevents::EventBuilder;

/// Represents a management event that has been received from the DB.
///
/// For management requests like [`crate::client::Client::ping`] and [`crate::client::Client::verify_api_token`] the DB will send a management event.
///
/// Compared to a normal Event, this does not contain the following fields:
/// - hash
/// - predecessorhash
/// - traceinfo
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementEvent {
    data: Value,
    datacontenttype: String,
    id: String,
    source: String,
    specversion: String,
    subject: String,
    time: DateTime<Utc>,
    r#type: String,
}

impl ManagementEvent {
    /// Get the data of an event.
    #[must_use]
    pub fn data(&self) -> &Value {
        &self.data
    }
    /// Get the data content type of an event.
    #[must_use]
    pub fn datacontenttype(&self) -> &str {
        &self.datacontenttype
    }
    /// Get the ID of an event.
    /// In eventsourcingdb, this is the sequence number of the event.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Get the source of an event.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Get the spec version of an event.
    /// This is always `1.0`.
    #[must_use]
    pub fn specversion(&self) -> &str {
        &self.specversion
    }
    /// Get the subject of an event.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
    /// Get the time of an event.
    #[must_use]
    pub fn time(&self) -> &DateTime<Utc> {
        &self.time
    }
    /// Get the type of an event.
    ///
    /// This method is called `ty` to avoid conflicts with the `type` keyword in Rust.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.r#type
    }
}

/// Optionally implement compatibility with the [cloudevents] crate.
#[cfg(feature = "cloudevents")]
impl From<ManagementEvent> for cloudevents::Event {
    fn from(event: ManagementEvent) -> Self {
        cloudevents::EventBuilderV10::new()
            .source(event.source)
            .subject(event.subject)
            .ty(event.r#type)
            .id(event.id)
            .time(event.time.to_string())
            .data(event.datacontenttype, event.data)
            .build()
            .expect("Failed to build cloudevent")
    }
}

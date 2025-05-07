//! This models events and event candidates for the DB.
//!
//! It also provides optional compatibility to the [cloudevents] crate (enable the `cloudevents` feature to get this).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use typed_builder::TypedBuilder;

#[cfg(feature = "cloudevents")]
use cloudevents::{AttributesReader, EventBuilder};
#[cfg(feature = "cloudevents")]
use crate::error::EventError;

/// Represents an event candidate that can be sent to the DB.
/// This is a simplified version of the [Event] type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TypedBuilder)]
pub struct EventCandidate {
    /// The data of the event, serialized as JSON
    #[builder(setter(into))]
    pub data: Value,
    /// The source of the event.
    /// This has to be a valid URI.
    #[builder(setter(into))]
    pub source: String,
    /// The subject of the event.
    /// This has to start with a `/`.
    #[builder(setter(into))]
    pub subject: String,
    /// The type of the event.
    /// This has to be a reverse domain name.
    #[builder(setter(into))]
    pub r#type: String,
    /// The traceparent of the event.
    /// This is used for distributed tracing.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
}

#[cfg(feature = "cloudevents")]
impl TryFrom<cloudevents::Event> for EventCandidate {
    type Error = EventError;
    fn try_from(event: cloudevents::Event) -> Result<Self, Self::Error> {
        let data = match event.data() {
            Some(cloudevents::Data::Json(json)) => json.to_owned(),
            _ => return Err(EventError::InvalidCloudevent),
        };
        let subject = match event.subject() {
            Some(subject) => subject.to_string(),
            None => return Err(EventError::InvalidCloudevent),
        };

        Ok(Self {
            data,
            source: event.source().to_string(),
            subject,
            r#type: event.ty().to_string(),
            traceparent: event.extension("traceparent").map(ToString::to_string),
        })
    }
}

/// Represents an event that has been received from the DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    data: Value,
    datacontenttype: String,
    hash: String,
    id: String,
    predecessorhash: String,
    source: String,
    specversion: String,
    subject: String,
    #[serde(with = "time::serde::iso8601")]
    time: OffsetDateTime,
    traceparent: Option<String>,
    tracestate: Option<String>,
    r#type: String,
}

impl Event {
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
    /// Get the hash of an event.
    #[must_use]
    pub fn hash(&self) -> &str {
        &self.hash
    }
    /// Get the ID of an event.
    /// In eventsourcingdb, this is the sequence number of the event.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Get the predecessor hash of an event.
    #[must_use]
    pub fn predecessorhash(&self) -> &str {
        &self.predecessorhash
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
    pub fn time(&self) -> &OffsetDateTime {
        &self.time
    }
    /// Get the traceparent of an event.
    #[must_use]
    pub fn traceparent(&self) -> Option<&str> {
        self.traceparent.as_deref()
    }
    /// Get the tracestate of an event.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        self.tracestate.as_deref()
    }
    /// Get the type of an event.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.r#type
    }
}

impl From<Event> for EventCandidate {
    fn from(event: Event) -> Self {
        Self {
            data: event.data,
            source: event.source,
            subject: event.subject,
            r#type: event.r#type,
            traceparent: event.traceparent,
        }
    }
}

#[cfg(feature = "cloudevents")]
impl From<Event> for cloudevents::Event {
    fn from(event: Event) -> Self {
        let mut builder = cloudevents::EventBuilderV10::new()
            .source(event.source)
            .subject(event.subject)
            .ty(event.r#type)
            .id(event.id)
            .time(event.time.to_string())
            .data(event.datacontenttype, event.data);

        if let Some(traceparent) = event.traceparent {
            builder = builder.extension("traceparent", traceparent);
        }

        builder.build().expect("Failed to build cloudevent")
    }
}

/// Represents a management event that has been received from the DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementEvent {
    data: Value,
    datacontenttype: String,
    id: String,
    source: String,
    specversion: String,
    subject: String,
    #[serde(with = "time::serde::iso8601")]
    time: OffsetDateTime,
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
    pub fn time(&self) -> &OffsetDateTime {
        &self.time
    }
    /// Get the type of an event.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.r#type
    }
}

impl From<ManagementEvent> for EventCandidate {
    fn from(event: ManagementEvent) -> Self {
        Self {
            data: event.data,
            source: event.source,
            subject: event.subject,
            r#type: event.r#type,
            traceparent: None,
        }
    }
}

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

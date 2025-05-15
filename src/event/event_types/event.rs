use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{trace_info::TraceInfo, EventCandidate};


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
    time: DateTime<Utc>,
    #[serde(flatten)]
    traceinfo: Option<TraceInfo>,
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
    pub fn time(&self) -> &DateTime<Utc> {
        &self.time
    }
    /// Get the traceparent of an event.
    #[must_use]
    pub fn traceparent(&self) -> Option<&str> {
        self.traceinfo.as_ref().map(TraceInfo::traceparent)
    }
    /// Get the tracestate of an event.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        self.traceinfo.as_ref().and_then(|t| t.tracestate())
    }
    /// Get the traceinfo of an event.
    #[must_use]
    pub fn traceinfo(&self) -> Option<&TraceInfo> {
        self.traceinfo.as_ref()
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
            traceinfo: event.traceinfo,
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

        if let Some(traceinfo) = event.traceinfo {
            builder = builder.extension("traceparent", traceinfo.traceparent());
            if let Some(tracestate) = traceinfo.tracestate() {
                builder = builder.extension("tracestate", tracestate);
            }
        }

        builder.build().expect("Failed to build cloudevent")
    }
}

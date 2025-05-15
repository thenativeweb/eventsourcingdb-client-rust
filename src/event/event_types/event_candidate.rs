use serde::{Deserialize, Serialize};
use serde_json::Value;
use typed_builder::TypedBuilder;
use crate::event::trace_info::TraceInfo;

#[cfg(feature = "cloudevents")]
use crate::error::EventError;

/// Represents an event candidate that can be sent to the DB.
/// This is a simplified version of the [`super::Event`] type.
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
    #[builder(default, setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub traceinfo: Option<TraceInfo>,
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
        let traceinfo = TraceInfo::from_cloudevent(&event)?;

        Ok(Self {
            data,
            source: event.source().to_string(),
            subject,
            r#type: event.ty().to_string(),
            traceinfo,
        })
    }
}

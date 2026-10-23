use super::{ClientRequest, OneShotRequest};
use crate::{
    client::Precondition,
    error::ClientError,
    event::{Event, EventCandidate},
};
use reqwest::Method;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WriteEventsRequest {
    pub events: Vec<EventCandidate>,
    pub preconditions: Vec<V1Precondition>,
}

/// A precondition as the v1 API takes it, which knows no options
#[allow(
    clippy::enum_variant_names,
    reason = "The variants are named as the variants of Precondition"
)]
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum V1Precondition {
    #[serde(rename = "isSubjectPristine")]
    IsSubjectPristine { subject: String },
    #[serde(rename = "isSubjectPopulated")]
    IsSubjectPopulated { subject: String },
    #[serde(rename = "isSubjectOnEventId")]
    IsSubjectOnEventId {
        subject: String,
        #[serde(rename = "eventId")]
        event_id: String,
    },
    #[serde(rename = "isEventQlQueryTrue")]
    IsEventQlQueryTrue { query: String },
}

impl TryFrom<Precondition> for V1Precondition {
    type Error = ClientError;

    /// Turns the precondition into one that the v1 API takes, leaving out its options.
    ///
    /// # Errors
    /// Returns [`ClientError::IncompatibleServer`] for a recursive precondition, since the v1 API can only check the
    /// subject alone, which would check less than was asked.
    fn try_from(precondition: Precondition) -> Result<Self, Self::Error> {
        if precondition.is_recursive() {
            return Err(ClientError::IncompatibleServer);
        }

        match precondition {
            Precondition::IsSubjectPristine { subject, .. } => {
                Ok(Self::IsSubjectPristine { subject })
            }
            Precondition::IsSubjectPopulated { subject, .. } => {
                Ok(Self::IsSubjectPopulated { subject })
            }
            Precondition::IsSubjectOnEventId {
                subject, event_id, ..
            } => Ok(Self::IsSubjectOnEventId { subject, event_id }),
            Precondition::IsEventQLQueryTrue { query, .. } => {
                Ok(Self::IsEventQlQueryTrue { query })
            }
        }
    }
}

impl ClientRequest for WriteEventsRequest {
    const URL_PATH: &'static str = "/api/v1/write-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl OneShotRequest for WriteEventsRequest {
    type Response = Vec<Event>;
}

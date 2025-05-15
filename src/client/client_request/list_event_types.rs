use futures::{Stream, stream::StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventType {
    #[serde(rename = "eventType")]
    pub name: String,
    pub is_phantom: bool,
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventTypesRequest;

impl ClientRequest for ListEventTypesRequest {
    const URL_PATH: &'static str = "/api/v1/read-event-types";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl StreamingRequest for ListEventTypesRequest {
    type ItemType = EventType;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        #[derive(Deserialize, Debug)]
        #[serde(tag = "type", content = "payload", rename_all = "camelCase")]
        enum LineItem {
            Error { error: String },
            EventType(EventType),
        }

        impl From<LineItem> for Result<EventType, ClientError> {
            fn from(item: LineItem) -> Self {
                match item {
                    LineItem::Error { error } => Err(ClientError::DBError(error)),
                    LineItem::EventType(event_type) => Ok(event_type),
                }
            }
        }

        Self::lines_stream(response).map(|line| {
            let line = line?;
            let item: LineItem = serde_json::from_str(line.as_str())?;
            item.into()
        })
    }
}

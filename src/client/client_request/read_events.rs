use futures::{Stream, stream::StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::{client::request_options::ReadEventsRequestOptions, error::ClientError, event::Event};

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
pub struct ReadEventsRequest<'a> {
    pub subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ReadEventsRequestOptions<'a>>,
}

impl ClientRequest for ReadEventsRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/read-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl StreamingRequest for ReadEventsRequest<'_> {
    type ItemType = Event;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        #[derive(Deserialize, Debug)]
        #[serde(tag = "type", content = "payload", rename_all = "camelCase")]
        enum LineItem {
            Error { error: String },
            Event(Box<Event>),
        }

        impl From<LineItem> for Result<Event, ClientError> {
            fn from(item: LineItem) -> Self {
                match item {
                    LineItem::Error { error } => Err(ClientError::DBError(error)),
                    LineItem::Event(event_type) => Ok(*event_type),
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

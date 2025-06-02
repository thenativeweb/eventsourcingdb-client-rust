use futures::{Stream, StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};

type EventqlRow = serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct RunEventqlQueryRequest<'a> {
    pub query: &'a str,
}

impl ClientRequest for RunEventqlQueryRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/run-eventql-query";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl StreamingRequest for RunEventqlQueryRequest<'_> {
    type ItemType = EventqlRow;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        #[derive(Deserialize, Debug)]
        #[serde(tag = "type", content = "payload", rename_all = "camelCase")]
        enum LineItem {
            Error { error: String },
            Row(EventqlRow),
        }

        impl From<LineItem> for Result<EventqlRow, ClientError> {
            fn from(item: LineItem) -> Self {
                match item {
                    LineItem::Error { error } => Err(ClientError::DBError(error)),
                    LineItem::Row(row) => Ok(row),
                }
            }
        }

        Self::lines_stream(response).map(|line| {
            let line = line?;
            let item = serde_json::from_str(line.as_str())?;
            Ok(item)
        })
    }
}

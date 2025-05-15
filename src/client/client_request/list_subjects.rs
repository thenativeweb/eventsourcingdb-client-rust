use futures::{Stream, stream::StreamExt};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSubjectsRequest<'a> {
    pub base_subject: &'a str,
}

impl<'a> ClientRequest for ListSubjectsRequest<'a> {
    const URL_PATH: &'static str = "/api/v1/read-subjects";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl<'a> StreamingRequest for ListSubjectsRequest<'a> {
    type ItemType = String;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        #[derive(Deserialize, Debug)]
        struct LineItem {
            payload: LineItemPayload,
            r#type: String,
        }
        #[derive(Deserialize, Debug)]
        struct LineItemPayload {
            subject: String,
        }
        Self::lines_stream(response).map(|line| {
            let line = line?;
            let item: LineItem = serde_json::from_str(line.as_str())?;
            if item.r#type != "subject" {
                return Err(ClientError::InvalidEventType);
            }
            Ok(item.payload.subject)
        })
    }
}

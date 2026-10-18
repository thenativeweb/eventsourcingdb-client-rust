use reqwest::Method;

use crate::{error::ClientError, event::ManagementEvent};

use super::{ClientRequest, OneShotRequest};

/// Ping the Database instance
#[derive(Debug, Clone, Copy)]
pub struct PingRequest;

impl ClientRequest for PingRequest {
    const URL_PATH: &'static str = "/api/v1/ping";
    const METHOD: Method = Method::GET;
}

impl OneShotRequest for PingRequest {
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.ping-received")
            .then_some(())
            .ok_or(ClientError::PingFailed)
    }
}

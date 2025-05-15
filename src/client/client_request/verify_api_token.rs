use reqwest::Method;

use crate::{error::ClientError, event::ManagementEvent};

use super::{ClientRequest, OneShotRequest};

/// Verify the API token
#[derive(Debug, Clone, Copy)]
pub struct VerifyApiTokenRequest;

impl ClientRequest for VerifyApiTokenRequest {
    const URL_PATH: &'static str = "/api/v1/verify-api-token";
    const METHOD: Method = Method::POST;
}

impl OneShotRequest for VerifyApiTokenRequest {
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.api-token-verified")
            .then_some(())
            .ok_or(ClientError::APITokenInvalid)
    }
}

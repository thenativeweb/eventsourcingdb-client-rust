//! This is a purely internal module that decides whether a write that failed may have been stored nonetheless.
//!
//! EventSourcingDB never stores a write halfway, but a write whose answer is lost may have been stored. A write is
//! certainly not stored only if the connection could not be established at all, or if the server refused it before
//! storing anything. Every other failure leaves the outcome unknown, so it is wrapped in
//! [`ClientError::OutcomeUnknown`].

use crate::error::{ClientError, DbApiError};

/// Wraps the error of a write that failed in [`ClientError::OutcomeUnknown`], unless the write was certainly not
/// stored: if the connection could not be established, or if the server refused the write, as `is_refusal` tells for
/// the version of the API.
///
/// The write must have been built already, so that the error is one of sending it or of its answer.
pub fn of_write<T>(
    result: Result<T, ClientError>,
    is_refusal: fn(&DbApiError) -> bool,
) -> Result<T, ClientError> {
    result.map_err(|error| {
        let is_certainly_not_stored = match &error {
            ClientError::ReqwestError(error) => error.is_connect(),
            // Only EventSourcingDB itself can refuse a write.
            ClientError::DbApiError(error) => error.is_from_eventsourcingdb && is_refusal(error),
            _ => false,
        };
        if is_certainly_not_stored {
            error
        } else {
            ClientError::OutcomeUnknown(Box::new(error))
        }
    })
}

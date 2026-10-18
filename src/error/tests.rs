use std::{error::Error, io};

use super::ClientError;

#[test]
fn an_io_error_names_its_cause_as_its_source() {
    let error = ClientError::from(io::Error::other("the connection was reset"));

    assert!(matches!(error, ClientError::IoError(_)));
    let source = error.source().expect("Expected the IO error as the source");
    assert_eq!(source.to_string(), "the connection was reset");
}

#[test]
fn a_db_api_error_leaves_out_whether_it_is_from_eventsourcingdb() {
    use reqwest::StatusCode;

    use super::DbApiError;

    let error = DbApiError::from_status(StatusCode::BAD_GATEWAY, "bad gateway", None);
    let error_of_a_proxy = error.clone().not_from_eventsourcingdb();

    assert_eq!(error, error_of_a_proxy);
    let debug = format!("{error_of_a_proxy:?}");
    assert!(
        !debug.contains("is_from_eventsourcingdb"),
        "Expected the debug output to leave out the origin, but got {debug}"
    );
    assert_eq!(
        debug,
        r#"DbApiError { code: "", description: "bad gateway", details: {}, status: 502, retry_after: None }"#
    );
}

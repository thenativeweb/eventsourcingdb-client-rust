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

#[test]
fn a_client_error_can_be_cloned_and_shares_the_error_it_wraps() {
    let error = ClientError::from(io::Error::other("the connection was reset"));
    let clone = error.clone();

    match (&error, &clone) {
        (ClientError::IoError(error), ClientError::IoError(clone)) => {
            assert!(std::sync::Arc::ptr_eq(error, clone));
        }
        _ => panic!("Expected two IO errors, but got {error:?} and {clone:?}"),
    }
    assert_eq!(clone.to_string(), error.to_string());
}

/// Finds an error of the given type along the sources of the error, as error handlers such as anyhow do.
fn find_source<T: Error + 'static>(error: &ClientError) -> Option<&T> {
    let mut source = error.source();
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<T>() {
            return Some(error);
        }
        source = error.source();
    }
    None
}

#[test]
fn the_sources_of_a_client_error_contain_the_errors_it_wraps() {
    let error = ClientError::from(io::Error::other("the connection was reset"));
    let io_error =
        find_source::<io::Error>(&error).expect("Expected the IO error among the sources");
    assert_eq!(io_error.to_string(), "the connection was reset");

    let reqwest_error = reqwest::Client::new()
        .get("http://localhost/")
        .header("invalid\nname", "value")
        .build()
        .expect_err("Expected the request to be invalid");
    let error = ClientError::from(reqwest_error);
    assert!(
        find_source::<reqwest::Error>(&error).is_some(),
        "Expected the reqwest error among the sources"
    );

    let serde_error = serde_json::from_str::<serde_json::Value>("no json").unwrap_err();
    let error = ClientError::from(serde_error);
    assert!(
        find_source::<serde_json::Error>(&error).is_some(),
        "Expected the serde error among the sources"
    );

    // An unknown outcome names the error the write failed with, and that its own.
    let error = ClientError::OutcomeUnknown(Box::new(ClientError::from(io::Error::other(
        "the connection was reset",
    ))));
    assert!(find_source::<ClientError>(&error).is_some());
    assert!(find_source::<io::Error>(&error).is_some());
}

use std::{error::Error, io};

use super::ClientError;

#[test]
fn an_io_error_names_its_cause_as_its_source() {
    let error = ClientError::from(io::Error::other("the connection was reset"));

    assert!(matches!(error, ClientError::IoError(_)));
    let source = error.source().expect("Expected the IO error as the source");
    assert_eq!(source.to_string(), "the connection was reset");
}

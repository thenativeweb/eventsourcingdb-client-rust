//! The fixtures are events exactly as EventSourcingDB returned them: the ones of version 1 from version 1.2.0, the
//! ones of version 2 from the preview of version 1.3.

use ed25519_dalek::SigningKey;

use super::Event;
use crate::error::EventError;

/// An event of version 1 without trace information
const V1_EVENT: &str = r#"{"source":"https://library.eventsourcingdb.io","subject":"/books/42","type":"io.eventsourcingdb.library.book-acquired","specversion":"1.0","id":"0","time":"2026-10-18T13:47:38.410062423Z","datacontenttype":"application/json","predecessorhash":"0000000000000000000000000000000000000000000000000000000000000000","data":{"author":"Arthur C. Clarke","isbn":"978-0756906788","pages":297,"title":"2001 – A Space Odyssey"},"hash":"a3cd9c40afb9361373edb8fd7456bb504c94a8ce5f3037faa367033443000996","signature":null}"#;
/// An event of version 1 with `traceparent` and `tracestate`, which version 1 does not cover
const V1_EVENT_WITH_TRACE_INFO: &str = r#"{"source":"https://library.eventsourcingdb.io","subject":"/books/42","type":"io.eventsourcingdb.library.book-borrowed","traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01","tracestate":"vendor=value","specversion":"1.0","id":"1","time":"2026-10-18T13:47:38.410062423Z","datacontenttype":"application/json","predecessorhash":"a3cd9c40afb9361373edb8fd7456bb504c94a8ce5f3037faa367033443000996","data":{"borrower":"/readers/23"},"hash":"3a9156f6b890ae46e2a203c878742133b67686cf6bd8b6e8c2649af2ccd7caa1","signature":null}"#;
/// An event of version 2 without trace information, so both `traceparent` and `tracestate` are missing
const V2_EVENT: &str = r#"{"source":"https://library.eventsourcingdb.io","subject":"/books/42","type":"io.eventsourcingdb.library.book-acquired","specversion":"1.0","id":"0","time":"2026-10-18T13:47:38.717047673Z","datacontenttype":"application/json","predecessorhash":"0000000000000000000000000000000000000000000000000000000000000000","data":{"author":"Arthur C. Clarke","isbn":"978-0756906788","pages":297,"title":"2001 – A Space Odyssey"},"hash":"esdb:hash:v2:3d63b75e107cb78d9f6c4b3556a4759f8bfa0130d52a5cec7c37ff414114a909","signature":null}"#;
/// An event of version 2 with `traceparent` and `tracestate`
const V2_EVENT_WITH_TRACE_INFO: &str = r#"{"source":"https://library.eventsourcingdb.io","subject":"/books/42","type":"io.eventsourcingdb.library.book-borrowed","traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01","tracestate":"vendor=value","specversion":"1.0","id":"1","time":"2026-10-18T13:47:38.717047673Z","datacontenttype":"application/json","predecessorhash":"esdb:hash:v2:3d63b75e107cb78d9f6c4b3556a4759f8bfa0130d52a5cec7c37ff414114a909","data":{"borrower":"/readers/23"},"hash":"esdb:hash:v2:5dd01d299b0cd74be15e861c2ffaffdbebd5dec4daf59e0aeacc2de3919d72aa","signature":null}"#;
/// An event of version 2 with a `traceparent`, but without a `tracestate`
const V2_EVENT_WITH_TRACEPARENT: &str = r#"{"source":"https://library.eventsourcingdb.io","subject":"/books/42","type":"io.eventsourcingdb.library.book-returned","traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01","specversion":"1.0","id":"2","time":"2026-10-18T13:47:38.717047673Z","datacontenttype":"application/json","predecessorhash":"esdb:hash:v2:5dd01d299b0cd74be15e861c2ffaffdbebd5dec4daf59e0aeacc2de3919d72aa","data":{},"hash":"esdb:hash:v2:ef4714fbca2006ec2f7d7d8ae07538d0c40409ddf7c720d2f60335ea5bb57d16","signature":null}"#;

/// The hash of [`V2_EVENT`]
const V2_EVENT_HASH: &str =
    "esdb:hash:v2:3d63b75e107cb78d9f6c4b3556a4759f8bfa0130d52a5cec7c37ff414114a909";
/// A hash whose prefix names a version that does not exist
const UNSUPPORTED_HASH: &str =
    "esdb:hash:v3:3d63b75e107cb78d9f6c4b3556a4759f8bfa0130d52a5cec7c37ff414114a909";

fn parse(event: &str) -> Event {
    serde_json::from_str(event).expect("Failed to parse the fixture")
}

/// Parses the fixture after replacing a part of it, which must occur exactly once.
fn parse_with(event: &str, from: &str, to: &str) -> Event {
    assert_eq!(event.matches(from).count(), 1, "Expected {from} once");
    parse(&event.replace(from, to))
}

fn assert_hash_verification_failed(event: &Event) -> (String, String) {
    match event.verify_hash() {
        Err(EventError::HashVerificationFailed { expected, actual }) => (expected, actual),
        result => panic!("Expected a failed hash verification, but got {result:?}"),
    }
}

#[test]
fn verifies_a_valid_hash_of_version_1() {
    parse(V1_EVENT)
        .verify_hash()
        .expect("Hash verification failed");
    parse(V1_EVENT_WITH_TRACE_INFO)
        .verify_hash()
        .expect("Hash verification failed");
}

#[test]
fn verifies_a_valid_hash_of_version_2() {
    parse(V2_EVENT)
        .verify_hash()
        .expect("Hash verification failed");
    parse(V2_EVENT_WITH_TRACE_INFO)
        .verify_hash()
        .expect("Hash verification failed");
    parse(V2_EVENT_WITH_TRACEPARENT)
        .verify_hash()
        .expect("Hash verification failed");
}

#[test]
fn rejects_an_invalid_hash_of_version_1() {
    let event = parse_with(V1_EVENT, r#""pages":297"#, r#""pages":298"#);

    let (expected, actual) = assert_hash_verification_failed(&event);
    assert_eq!(expected, event.hash());
    assert_eq!(actual.len(), 64);
    assert_ne!(actual, expected);
    assert_eq!(
        EventError::HashVerificationFailed { expected, actual }.to_string(),
        "Hash verification failed"
    );
}

#[test]
fn rejects_an_invalid_hash_of_version_2() {
    let event = parse_with(V2_EVENT, r#""pages":297"#, r#""pages":298"#);

    let (expected, actual) = assert_hash_verification_failed(&event);
    assert_eq!(expected, event.hash());
    assert!(actual.starts_with("esdb:hash:v2:"));
    assert_ne!(actual, expected);
    assert_eq!(
        EventError::HashVerificationFailed { expected, actual }.to_string(),
        "Hash verification failed"
    );
}

#[test]
fn version_2_covers_the_trace_information() {
    let changed_traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
    let changed_tracestate = r#""tracestate":"vendor=other""#;

    // Version 1 does not cover the trace information, so changing it keeps the hash valid.
    parse_with(
        V1_EVENT_WITH_TRACE_INFO,
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        changed_traceparent,
    )
    .verify_hash()
    .expect("Hash verification failed");

    let _ = assert_hash_verification_failed(&parse_with(
        V2_EVENT_WITH_TRACE_INFO,
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        changed_traceparent,
    ));
    let _ = assert_hash_verification_failed(&parse_with(
        V2_EVENT_WITH_TRACE_INFO,
        r#""tracestate":"vendor=value""#,
        changed_tracestate,
    ));
    // A missing tracestate is hashed as `-`, so adding one changes the hash.
    let _ = assert_hash_verification_failed(&parse_with(
        V2_EVENT_WITH_TRACEPARENT,
        r#""specversion""#,
        &format!("{changed_tracestate},\"specversion\""),
    ));
}

#[test]
fn version_2_hashes_data_byte_for_byte() {
    // The same data with a space added is equal as JSON, but not as the bytes that arrived.
    let event = parse_with(
        V2_EVENT_WITH_TRACE_INFO,
        r#"{"borrower":"/readers/23"}"#,
        r#"{"borrower": "/readers/23"}"#,
    );

    let _ = assert_hash_verification_failed(&event);
}

#[test]
fn rejects_a_hash_of_an_unsupported_version() {
    let event = parse_with(V2_EVENT, V2_EVENT_HASH, UNSUPPORTED_HASH);

    let result = event.verify_hash();
    assert!(
        matches!(&result, Err(EventError::UnsupportedHashVersion { version }) if version == "v3"),
        "Expected an unsupported hash version, but got {result:?}"
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "The hash version 'v3' is not supported"
    );
}

#[test]
fn treats_a_hash_with_an_incomplete_prefix_as_version_1() {
    let event = parse_with(V2_EVENT, V2_EVENT_HASH, "esdb:hash:v2");

    let (_, actual) = assert_hash_verification_failed(&event);
    assert_eq!(actual.len(), 64);
}

#[test]
fn signature_verification_fails_for_a_hash_of_an_unsupported_version() {
    let verifying_key = SigningKey::from_bytes(&[7; 32]).verifying_key();
    let signature = format!("esdb:signature:v1:{}", "0".repeat(128));
    let event = parse_with(
        &V2_EVENT.replace(V2_EVENT_HASH, UNSUPPORTED_HASH),
        r#""signature":null"#,
        &format!(r#""signature":"{signature}""#),
    );

    let result = event.verify_signature(&verifying_key);
    assert!(
        matches!(&result, Err(EventError::UnsupportedHashVersion { version }) if version == "v3"),
        "Expected an unsupported hash version, but got {result:?}"
    );
}

#[test]
fn signature_verification_fails_for_an_invalid_signature() {
    let verifying_key = SigningKey::from_bytes(&[7; 32]).verifying_key();
    let signature = format!("esdb:signature:v1:{}", "0".repeat(128));
    let event = parse_with(
        V2_EVENT,
        r#""signature":null"#,
        &format!(r#""signature":"{signature}""#),
    );

    let result = event.verify_signature(&verifying_key);
    assert!(
        matches!(&result, Err(EventError::SignatureVerificationFailed(_))),
        "Expected a failed signature verification, but got {result:?}"
    );
}

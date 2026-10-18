//! Constants for the codes of the errors that the server reports, see [`DbApiError::code`](super::DbApiError::code).
//!
//! Each code is the URL of the page that documents the error, exactly as the server sends it, so that a caller can
//! tell errors apart without comparing descriptions:
//!
//! ```
//! use eventsourcingdb::error::{ClientError, code};
//!
//! fn is_precondition_violated(error: &ClientError) -> bool {
//!     matches!(error, ClientError::DbApiError(error) if error.code == code::PRECONDITION_VIOLATED)
//! }
//! ```

/// Defines a constant for each code, named after its slug, and, for the tests, the list of all of them.
macro_rules! codes {
    ($($name:ident = $slug:literal;)*) => {
        $(
            #[doc = concat!("The code of the error `", $slug, "`.")]
            pub const $name: &str = concat!("https://docs.eventsourcingdb.io/errors/", $slug);
        )*

        /// All codes, in the order in which the server lists them
        #[cfg(test)]
        pub(crate) const ALL: &[&str] = &[$($name),*];
    };
}

codes! {
    SUBJECT_MISSING = "subject-missing";
    SUBJECT_MALFORMED = "subject-malformed";
    ORDER_UNKNOWN = "order-unknown";
    EVENT_ID_MISSING = "event-id-missing";
    EVENT_ID_MALFORMED = "event-id-malformed";
    BOUND_TYPE_MISSING = "bound-type-missing";
    BOUND_TYPE_UNKNOWN = "bound-type-unknown";
    RANGE_UNSATISFIABLE = "range-unsatisfiable";
    FROM_LATEST_EVENT_INCOMPLETE = "from-latest-event-incomplete";
    IF_EVENT_IS_MISSING_UNKNOWN = "if-event-is-missing-unknown";
    SELECTION_CONFLICTING = "selection-conflicting";
    EVENT_TYPE_MISSING = "event-type-missing";
    EVENT_TYPE_MALFORMED = "event-type-malformed";
    EVENT_TYPE_UNKNOWN = "event-type-unknown";
    SOURCE_MISSING = "source-missing";
    SOURCE_MALFORMED = "source-malformed";
    DATA_MISSING = "data-missing";
    TRACEPARENT_MALFORMED = "traceparent-malformed";
    TRACESTATE_MALFORMED = "tracestate-malformed";
    TRACESTATE_WITHOUT_TRACEPARENT = "tracestate-without-traceparent";
    EVENTS_MISSING = "events-missing";
    PRECONDITION_TYPE_MISSING = "precondition-type-missing";
    PRECONDITION_UNKNOWN = "precondition-unknown";
    PAYLOAD_MISSING = "payload-missing";
    LICENSE_INVALID = "license-invalid";
    CAPACITY_EXCEEDED = "capacity-exceeded";
    EVENT_SCHEMA_VIOLATED = "event-schema-violated";
    EVENT_SCHEMA_UNREGISTERED = "event-schema-unregistered";
    EVENT_SCHEMA_MISSING = "event-schema-missing";
    EVENT_SCHEMA_MALFORMED = "event-schema-malformed";
    EVENT_SCHEMA_NOT_OBJECT = "event-schema-not-object";
    EVENT_SCHEMA_ALREADY_REGISTERED = "event-schema-already-registered";
    LAST_AUDITED_EVENT_MISSING = "last-audited-event-missing";
    LAST_AUDITED_EVENT_MISMATCHED = "last-audited-event-mismatched";
    LAST_AUDITED_EVENT_UNKNOWN = "last-audited-event-unknown";
    EVENT_HASH_MISSING = "event-hash-missing";
    EVENT_HASH_MALFORMED = "event-hash-malformed";
    EVENT_CHAIN_BROKEN = "event-chain-broken";
    LAST_BACKED_UP_EVENT_MISSING = "last-backed-up-event-missing";
    LAST_BACKED_UP_EVENT_MISMATCHED = "last-backed-up-event-mismatched";
    LAST_BACKED_UP_EVENT_UNKNOWN = "last-backed-up-event-unknown";
    STORE_NOT_EMPTY = "store-not-empty";
    EVENT_HASH_VERSION_UNKNOWN = "event-hash-version-unknown";
    BACKUP_MALFORMED = "backup-malformed";
    BACKUP_INCOMPLETE = "backup-incomplete";
    BACKUP_FORMAT_UNKNOWN = "backup-format-unknown";
    FULL_BACKUP_MISSING = "full-backup-missing";
    BACKUP_INCREMENT_DISCONNECTED = "backup-increment-disconnected";
    BACKUP_FAILED = "backup-failed";
    BACKUP_UNREADABLE = "backup-unreadable";
    EVENT_TOO_LARGE = "event-too-large";
    DATA_MALFORMED = "data-malformed";
    QUERY_MISSING = "query-missing";
    QUERY_MALFORMED = "query-malformed";
    QUERY_UNEVALUABLE = "query-unevaluable";
    QUERY_NOT_BOOLEAN = "query-not-boolean";
    PRECONDITION_VIOLATED = "precondition-violated";
    STORE_READ_ONLY = "store-read-only";
    READ_ONLY_MODE_ALREADY_ENABLED = "read-only-mode-already-enabled";
    DISK_FULL = "disk-full";
    RESTORE_IN_PROGRESS = "restore-in-progress";
    STORE_FAILED = "store-failed";
    INDEXES_REBUILDING = "indexes-rebuilding";
    SERVER_SHUTTING_DOWN = "server-shutting-down";
}

#[cfg(test)]
mod tests {
    /// The slugs of the errors in the order in which the server lists them, in `cmd/server/api/error.go` of
    /// EventSourcingDB, as of revision `36fa882`
    const SERVER_SLUGS: &[&str] = &[
        "subject-missing",
        "subject-malformed",
        "order-unknown",
        "event-id-missing",
        "event-id-malformed",
        "bound-type-missing",
        "bound-type-unknown",
        "range-unsatisfiable",
        "from-latest-event-incomplete",
        "if-event-is-missing-unknown",
        "selection-conflicting",
        "event-type-missing",
        "event-type-malformed",
        "event-type-unknown",
        "source-missing",
        "source-malformed",
        "data-missing",
        "traceparent-malformed",
        "tracestate-malformed",
        "tracestate-without-traceparent",
        "events-missing",
        "precondition-type-missing",
        "precondition-unknown",
        "payload-missing",
        "license-invalid",
        "capacity-exceeded",
        "event-schema-violated",
        "event-schema-unregistered",
        "event-schema-missing",
        "event-schema-malformed",
        "event-schema-not-object",
        "event-schema-already-registered",
        "last-audited-event-missing",
        "last-audited-event-mismatched",
        "last-audited-event-unknown",
        "event-hash-missing",
        "event-hash-malformed",
        "event-chain-broken",
        "last-backed-up-event-missing",
        "last-backed-up-event-mismatched",
        "last-backed-up-event-unknown",
        "store-not-empty",
        "event-hash-version-unknown",
        "backup-malformed",
        "backup-incomplete",
        "backup-format-unknown",
        "full-backup-missing",
        "backup-increment-disconnected",
        "backup-failed",
        "backup-unreadable",
        "event-too-large",
        "data-malformed",
        "query-missing",
        "query-malformed",
        "query-unevaluable",
        "query-not-boolean",
        "precondition-violated",
        "store-read-only",
        "read-only-mode-already-enabled",
        "disk-full",
        "restore-in-progress",
        "store-failed",
        "indexes-rebuilding",
        "server-shutting-down",
    ];

    #[test]
    fn there_is_a_constant_for_every_code_of_the_server() {
        let expected: Vec<String> = SERVER_SLUGS
            .iter()
            .map(|slug| format!("https://docs.eventsourcingdb.io/errors/{slug}"))
            .collect();

        assert_eq!(super::ALL, expected);
    }

    #[test]
    fn the_constants_are_named_after_their_slug() {
        assert_eq!(
            super::PRECONDITION_VIOLATED,
            "https://docs.eventsourcingdb.io/errors/precondition-violated"
        );
        assert_eq!(
            super::TRACESTATE_WITHOUT_TRACEPARENT,
            "https://docs.eventsourcingdb.io/errors/tracestate-without-traceparent"
        );
    }
}

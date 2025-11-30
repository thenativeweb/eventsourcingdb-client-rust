//! Polars DataFrame integration for EventSourcingDB events.
//!
//! This module provides utilities to convert event streams into Polars DataFrames
//! for data analysis and exploration.
//!
//! # Example
//!
//! ```rust,ignore
//! use eventsourcingdb::polars::events_to_dataframe;
//! use futures::StreamExt;
//!
//! let events = client.read_events("/", None).await?;
//! let df = events_to_dataframe(events).await?;
//! println!("{}", df);
//! ```

use futures::{Stream, StreamExt};
use polars::prelude::*;

use crate::error::ClientError;
use crate::event::Event;

/// Convert a stream of events to a Polars DataFrame.
///
/// All event fields are included as columns. The `data` field is stored as a
/// JSON string - use Polars' `str().json_path_match()` for extraction if needed.
///
/// # Arguments
///
/// * `events` - A stream of Event results from read_events or observe_events
///
/// # Returns
///
/// A Polars DataFrame with all event fields as columns:
/// - `event_id`: String
/// - `time`: Datetime (milliseconds)
/// - `source`: String
/// - `subject`: String
/// - `type`: String
/// - `data`: String (JSON)
/// - `spec_version`: String
/// - `data_content_type`: String
/// - `predecessor_hash`: String
/// - `hash`: String
/// - `trace_parent`: String (nullable)
/// - `trace_state`: String (nullable)
/// - `signature`: String (nullable)
///
/// # Errors
///
/// Returns a `PolarsError` if:
/// - The event stream produces an error
/// - DataFrame construction fails
pub async fn events_to_dataframe<S>(mut events: S) -> Result<DataFrame, PolarsError>
where
    S: Stream<Item = Result<Event, ClientError>> + Unpin,
{
    let mut event_ids: Vec<String> = Vec::new();
    let mut times: Vec<i64> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    let mut subjects: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    let mut data: Vec<String> = Vec::new();
    let mut spec_versions: Vec<String> = Vec::new();
    let mut data_content_types: Vec<String> = Vec::new();
    let mut predecessor_hashes: Vec<String> = Vec::new();
    let mut hashes: Vec<String> = Vec::new();
    let mut trace_parents: Vec<Option<String>> = Vec::new();
    let mut trace_states: Vec<Option<String>> = Vec::new();
    let mut signatures: Vec<Option<String>> = Vec::new();

    while let Some(result) = events.next().await {
        let event = result.map_err(|e| {
            PolarsError::ComputeError(format!("Failed to read event: {e}").into())
        })?;

        event_ids.push(event.id().to_string());
        times.push(event.time().timestamp_millis());
        sources.push(event.source().to_string());
        subjects.push(event.subject().to_string());
        types.push(event.ty().to_string());
        data.push(event.data().to_string());
        spec_versions.push(event.specversion().to_string());
        data_content_types.push(event.datacontenttype().to_string());
        predecessor_hashes.push(event.predecessorhash().to_string());
        hashes.push(event.hash().to_string());
        trace_parents.push(event.traceparent().map(ToString::to_string));
        trace_states.push(event.tracestate().map(ToString::to_string));
        signatures.push(event.signature().map(ToString::to_string));
    }

    DataFrame::new(vec![
        Column::new("event_id".into(), event_ids),
        Column::new("time".into(), times)
            .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))?,
        Column::new("source".into(), sources),
        Column::new("subject".into(), subjects),
        Column::new("type".into(), types),
        Column::new("data".into(), data),
        Column::new("spec_version".into(), spec_versions),
        Column::new("data_content_type".into(), data_content_types),
        Column::new("predecessor_hash".into(), predecessor_hashes),
        Column::new("hash".into(), hashes),
        Column::new("trace_parent".into(), trace_parents),
        Column::new("trace_state".into(), trace_states),
        Column::new("signature".into(), signatures),
    ])
}

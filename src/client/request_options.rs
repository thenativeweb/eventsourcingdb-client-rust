//! This module contains supporting options for the client requests.

use serde::Serialize;

/// Options for reading events from the database
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadEventsRequestOptions<'a> {
    /// Start reading events from this start event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_latest_event: Option<FromLatestEventOptions<'a>>,
    /// Lower bound of events to read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<Bound<'a>>,
    /// Ordering of the returned events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<Ordering>,
    /// Include recursive subject's events
    pub recursive: bool,
    /// Upper bound of events to read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upper_bound: Option<Bound<'a>>,
}

/// Options for observing events from the database
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveEventsRequestOptions<'a> {
    /// Start reading events from this start event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_latest_event: Option<FromLatestEventOptions<'a>>,
    /// Lower bound of events to read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<Bound<'a>>,
    /// Include recursive subject's events
    pub recursive: bool,
}

/// Ordering of the responses of requests
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ordering {
    /// Order the responses in chronological order
    Chronological,
    /// Order the responses in reverse chronological order
    Antichronological,
}

/// The type of the request bound
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundType {
    /// The bound is included in the response
    Inclusive,
    /// The bound is excluded from the response
    Exclusive,
}

/// A single bound for the request
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bound<'a> {
    /// The type of the bound
    #[serde(rename = "type")]
    pub bound_type: BoundType,
    /// The value of the bound
    pub id: &'a str,
}

/// The strategy for handling missing events
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventMissingStrategy {
    /// Read all events if the required one is missing
    ReadEverything,
    /// Read no events if the required one is missing
    ReadNothing,
}

/// Options for reading events from the start reading at
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FromLatestEventOptions<'a> {
    /// The strategy for handling missing events
    pub if_event_is_missing: EventMissingStrategy,
    /// The subject the event should be on
    pub subject: &'a str,
    /// The type of the event to read from
    #[serde(rename = "type")]
    pub ty: &'a str,
}

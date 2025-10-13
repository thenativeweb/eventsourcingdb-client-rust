use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::{RawValue, Value};

use crate::{
    error::EventError,
    event::{EventCandidate, trace_info::TraceInfo},
};
#[cfg(feature = "cloudevents")]
use cloudevents::EventBuilder;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct CustomValue {
    raw: Box<RawValue>,
    parsed: Value,
}

impl PartialEq for CustomValue {
    fn eq(&self, other: &Self) -> bool {
        self.parsed == other.parsed
    }
}

impl Eq for CustomValue {}

impl<'de> Deserialize<'de> for CustomValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // First get the raw JSON text
        let raw: Box<RawValue> = Deserialize::deserialize(deserializer)?;

        // Then parse it into `serde_json::Value`
        let parsed: Value = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;

        Ok(Self { raw, parsed })
    }
}

impl Serialize for CustomValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the raw value directly as JSON, not as a string
        self.raw.serialize(serializer)
    }
}

/// Represents an event that has been received from the DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    data: CustomValue,
    datacontenttype: String,
    hash: String,
    id: String,
    predecessorhash: String,
    source: String,
    specversion: String,
    subject: String,
    time: DateTime<Utc>,
    #[serde(flatten)]
    traceinfo: Option<TraceInfo>,
    #[serde(rename = "type")]
    ty: String,
    signature: Option<String>,
}

impl Event {
    /// Get the data of an event.
    #[must_use]
    pub fn data(&self) -> &Value {
        &self.data.parsed
    }
    /// Get the data content type of an event.
    #[must_use]
    pub fn datacontenttype(&self) -> &str {
        &self.datacontenttype
    }
    /// Get the hash of an event.
    #[must_use]
    pub fn hash(&self) -> &str {
        &self.hash
    }
    /// Get the ID of an event.
    /// In eventsourcingdb, this is the sequence number of the event.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Get the predecessor hash of an event.
    #[must_use]
    pub fn predecessorhash(&self) -> &str {
        &self.predecessorhash
    }
    /// Get the signature of an event.
    #[must_use]
    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }
    /// Get the source of an event.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Get the spec version of an event.
    /// This is always `1.0`.
    #[must_use]
    pub fn specversion(&self) -> &str {
        &self.specversion
    }
    /// Get the subject of an event.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }
    /// Get the time of an event.
    #[must_use]
    pub fn time(&self) -> &DateTime<Utc> {
        &self.time
    }
    /// Get the traceparent of an event.
    #[must_use]
    pub fn traceparent(&self) -> Option<&str> {
        self.traceinfo.as_ref().map(TraceInfo::traceparent)
    }
    /// Get the tracestate of an event.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        self.traceinfo.as_ref().and_then(|t| t.tracestate())
    }
    /// Get the traceinfo of an event.
    #[must_use]
    pub fn traceinfo(&self) -> Option<&TraceInfo> {
        self.traceinfo.as_ref()
    }
    /// Get the type of an event.
    #[must_use]
    pub fn ty(&self) -> &str {
        &self.ty
    }

    /// Verify the hash of an event.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_preview().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let candidates = vec![
    ///     EventCandidate::builder()
    ///        .source("https://www.eventsourcingdb.io".to_string())
    ///        .data(json!({"value": 1}))
    ///        .subject("/test".to_string())
    ///        .ty("io.eventsourcingdb.test".to_string())
    ///        .build()
    /// ];
    /// let written_events = client.write_events(candidates, vec![]).await.expect("Failed to write events");
    /// let event = &written_events[0];
    /// event.verify_hash().expect("Hash verification failed");
    /// # })
    /// ```
    ///
    /// # Errors
    /// Returns an error if the hash verification fails.
    pub fn verify_hash(&self) -> Result<(), EventError> {
        let metadata = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.specversion,
            self.id,
            self.predecessorhash,
            self.time
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            self.source,
            self.subject,
            self.ty,
            self.datacontenttype,
        );

        let metadata_hash = Sha256::digest(metadata.as_bytes());
        let metadata_hash_hex = hex::encode(metadata_hash);

        let data_hash = Sha256::digest(self.data.raw.get());
        let data_hash_hex = hex::encode(data_hash);

        let final_hash_input = format!("{metadata_hash_hex}{data_hash_hex}");
        let final_hash = Sha256::digest(final_hash_input.as_bytes());
        let final_hash_hex = hex::encode(final_hash);

        if final_hash_hex == self.hash {
            Ok(())
        } else {
            Err(EventError::HashVerificationFailed {
                expected: self.hash.clone(),
                actual: final_hash_hex,
            })
        }
    }

    /// Verify the signature of an event.
    ///
    /// To do this, the hash of the event is verified first before checking the signature against that hash.
    ///
    /// # Errors
    /// Returns an error if the signature is missing or malformed, or if the signature
    /// verification fails.
    pub fn verify_signature(&self, public_key: &VerifyingKey) -> Result<(), EventError> {
        const SIGNATURE_PREFIX: &str = "esdb:signature:v1:";

        let Some(signature) = &self.signature else {
            return Err(EventError::MissingSignature);
        };
        self.verify_hash()?;

        let signature = signature
            .strip_prefix(SIGNATURE_PREFIX)
            .ok_or(EventError::MalformedSignature)?;

        let signature_bytes: [u8; 64] = hex::decode(signature)
            .map_err(|_| EventError::MalformedSignature)?
            .try_into()
            .map_err(|_| EventError::MalformedSignature)?;
        let signature = Signature::from_bytes(&signature_bytes);
        Ok(public_key.verify(self.hash.as_bytes(), &signature)?)
    }
}

impl From<Event> for EventCandidate {
    fn from(event: Event) -> Self {
        Self {
            data: event.data.parsed,
            source: event.source,
            subject: event.subject,
            ty: event.ty,
            traceinfo: event.traceinfo,
        }
    }
}

#[cfg(feature = "cloudevents")]
impl From<Event> for cloudevents::Event {
    fn from(event: Event) -> Self {
        let mut builder = cloudevents::EventBuilderV10::new()
            .source(event.source)
            .subject(event.subject)
            .ty(event.ty)
            .id(event.id)
            .time(event.time.to_string())
            .data(event.datacontenttype, event.data.parsed);

        if let Some(traceinfo) = event.traceinfo {
            builder = builder.extension("traceparent", traceinfo.traceparent());
            if let Some(tracestate) = traceinfo.tracestate() {
                builder = builder.extension("tracestate", tracestate);
            }
        }

        builder.build().expect("Failed to build cloudevent")
    }
}

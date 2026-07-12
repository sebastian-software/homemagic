use chrono::{DateTime, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::{MatterControllerEventId, MatterFabricId, MatterOperationId, MatterSubscriptionId};

use super::{MatterDescriptorRevision, MatterEndpointNumber, MatterNodeId, MatterOperationPhase};

/// Maximum UTF-8 bytes in one normalized Matter scalar.
pub const MAX_MATTER_TEXT_BYTES: usize = 1_024;
/// Maximum octets in one normalized Matter scalar.
pub const MAX_MATTER_OCTETS: usize = 4_096;

/// Bounded UTF-8 scalar from a controller report.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MatterText(String);

impl MatterText {
    /// Creates bounded UTF-8 report text.
    ///
    /// # Errors
    ///
    /// Rejects text longer than [`MAX_MATTER_TEXT_BYTES`].
    pub fn new(value: impl Into<String>) -> Result<Self, MatterValueError> {
        let value = value.into();
        if value.len() > MAX_MATTER_TEXT_BYTES {
            Err(MatterValueError::TextTooLong)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the bounded text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for MatterText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MatterText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Bounded octet scalar from a controller report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterOctets(Vec<u8>);

impl MatterOctets {
    /// Creates bounded report octets.
    ///
    /// # Errors
    ///
    /// Rejects values longer than [`MAX_MATTER_OCTETS`].
    pub fn new(value: impl Into<Vec<u8>>) -> Result<Self, MatterValueError> {
        let value = value.into();
        if value.len() > MAX_MATTER_OCTETS {
            Err(MatterValueError::OctetsTooLong)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the bounded bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for MatterOctets {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MatterOctets {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Vec::<u8>::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Bounded SDK-neutral scalar reported by a Matter controller.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MatterAttributeValue {
    /// Explicit null.
    Null,
    /// Boolean scalar.
    Boolean(bool),
    /// Unsigned integer scalar.
    Unsigned(u64),
    /// Signed integer scalar.
    Signed(i64),
    /// Bounded UTF-8 scalar.
    Text(MatterText),
    /// Bounded octet scalar.
    Octets(MatterOctets),
}

/// Stable attribute path within one Matter node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MatterAttributePath {
    /// Fabric-scoped node identity.
    pub node_id: MatterNodeId,
    /// Endpoint number.
    pub endpoint: MatterEndpointNumber,
    /// Numeric cluster identifier.
    pub cluster_id: u32,
    /// Numeric attribute identifier.
    pub attribute_id: u32,
}

/// One normalized attribute report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterAttributeReport {
    /// Reported attribute path.
    pub path: MatterAttributePath,
    /// Bounded SDK-neutral value.
    pub value: MatterAttributeValue,
    /// Optional cluster data version.
    pub data_version: Option<u32>,
    /// Adapter-normalized sequence for ordering and deduplication.
    pub report_sequence: u64,
    /// Source observation time.
    pub observed_at: DateTime<Utc>,
}

/// Stable reason a logical subscription stopped delivering safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterSubscriptionLossReason {
    /// Secure session was closed.
    SessionClosed,
    /// Report sequence or data-version gap was detected.
    ReportGap,
    /// Subscription deadline elapsed.
    TimedOut,
    /// Descriptor change invalidated paths.
    DescriptorChanged,
    /// Adapter restarted and must restore ephemeral state.
    ControllerRestarted,
}

/// Normalized controller event payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterControllerEventKind {
    /// One attribute report arrived.
    AttributeReport {
        /// Stable `HomeMagic` fabric identity.
        fabric_id: MatterFabricId,
        /// Normalized report.
        report: MatterAttributeReport,
    },
    /// One logical subscription became unsafe.
    SubscriptionLost {
        /// Logical subscription identity.
        subscription_id: MatterSubscriptionId,
        /// Stable loss reason.
        reason: MatterSubscriptionLossReason,
    },
    /// Descriptor assumptions changed for one node.
    DescriptorChanged {
        /// Stable `HomeMagic` fabric identity.
        fabric_id: MatterFabricId,
        /// Fabric-scoped node identity.
        node_id: MatterNodeId,
        /// New adapter-normalized descriptor revision.
        descriptor_revision: MatterDescriptorRevision,
    },
    /// Durable operation reached a new phase.
    OperationProgress {
        /// Stable operation identity.
        operation_id: MatterOperationId,
        /// New validated phase.
        phase: MatterOperationPhase,
    },
}

/// One normalized controller event independent from SDK callbacks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterControllerEvent {
    /// Stable event identity.
    pub id: MatterControllerEventId,
    /// Time `HomeMagic` normalized the event.
    pub occurred_at: DateTime<Utc>,
    /// Typed bounded event payload.
    pub kind: MatterControllerEventKind,
}

/// Invalid normalized Matter scalar.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MatterValueError {
    /// UTF-8 value exceeded the fixed bound.
    #[error("Matter text exceeds 1024 UTF-8 bytes")]
    TextTooLong,
    /// Octet value exceeded the fixed bound.
    #[error("Matter octets exceed 4096 bytes")]
    OctetsTooLong,
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn matter_text_should_reject_oversized_input() {
        let result = MatterText::new("x".repeat(MAX_MATTER_TEXT_BYTES + 1));

        assert_eq!(result, Err(MatterValueError::TextTooLong));
    }

    #[test]
    fn matter_octets_should_reject_oversized_deserialization() {
        let json = format!("[{}]", vec!["0"; MAX_MATTER_OCTETS + 1].join(","));

        let result = serde_json::from_str::<MatterOctets>(&json);

        assert!(result.is_err(), "oversized octets should be rejected");
    }

    #[test]
    fn controller_event_should_round_trip_through_json() -> Result<(), Box<dyn Error>> {
        let event = MatterControllerEvent {
            id: MatterControllerEventId::new(),
            occurred_at: Utc::now(),
            kind: MatterControllerEventKind::DescriptorChanged {
                fabric_id: MatterFabricId::new(),
                node_id: MatterNodeId::new(42)?,
                descriptor_revision: MatterDescriptorRevision::new(2)?,
            },
        };

        let encoded = serde_json::to_string(&event)?;
        let decoded = serde_json::from_str(&encoded)?;

        assert_eq!(event, decoded);
        Ok(())
    }
}

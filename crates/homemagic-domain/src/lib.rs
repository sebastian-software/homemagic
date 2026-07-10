//! Core `HomeMagic` domain types.
//!
//! The domain separates stable device identity from mutable names and models
//! behavior as composable capability snapshots.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const DEVICE_NAMESPACE: Uuid = Uuid::from_u128(0x91d0_41aa_328c_5ba1_aaf6_e116_81a1_0cc9);

/// Stable opaque identifier for a device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(Uuid);

impl DeviceId {
    /// Derives a stable ID from an adapter namespace and immutable native ID.
    #[must_use]
    pub fn from_native(integration: &str, native_id: &str) -> Self {
        let key = format!("{integration}:{native_id}");
        Self(Uuid::new_v5(&DEVICE_NAMESPACE, key.as_bytes()))
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DeviceId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// Stable adapter identity for an independently addressable device endpoint.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EndpointId(String);

impl EndpointId {
    /// Creates an endpoint ID from an adapter-owned stable component key.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// The operational risk associated with invoking a capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Read-only or non-physical operation.
    #[default]
    Observe,
    /// Reversible comfort operation, such as changing a light.
    Comfort,
    /// Physical movement that may require safety constraints.
    Mechanical,
    /// Security- or privacy-sensitive operation.
    Security,
}

/// Current state of one normalized capability.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilitySnapshot {
    /// Device or endpoint reachability.
    Availability {
        /// Whether the adapter currently considers the target reachable.
        online: bool,
    },
    /// Binary on/off state.
    OnOff {
        /// Current reported output state.
        on: bool,
        /// Risk applied to future state-changing commands.
        risk: RiskClass,
    },
    /// A normalized level in percent.
    Level {
        /// Current level from zero through one hundred.
        percent: f64,
        /// Risk applied to future level-changing commands.
        risk: RiskClass,
    },
    /// Position or motion of a physical opening or cover.
    Position {
        /// Current position, when calibrated and reported.
        percent: Option<f64>,
        /// Vendor-normalized motion state.
        motion: Option<String>,
        /// Risk applied to future movement commands.
        risk: RiskClass,
    },
    /// Instantaneous electrical measurements.
    Power {
        /// Active power in watts.
        watts: Option<f64>,
        /// Voltage in volts.
        volts: Option<f64>,
        /// Current in amperes.
        amperes: Option<f64>,
    },
    /// Accumulated electrical energy.
    Energy {
        /// Total energy in watt-hours.
        watt_hours: f64,
    },
    /// Adapter-provided diagnostic state.
    Diagnostics {
        /// Firmware version, when reported.
        firmware_version: Option<String>,
        /// Current device errors.
        errors: Vec<String>,
    },
}

impl CapabilitySnapshot {
    /// Returns the stable capability schema name.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        match self {
            Self::Availability { .. } => "availability.v1",
            Self::OnOff { .. } => "on_off.v1",
            Self::Level { .. } => "level.v1",
            Self::Position { .. } => "position.v1",
            Self::Power { .. } => "power.v1",
            Self::Energy { .. } => "energy.v1",
            Self::Diagnostics { .. } => "diagnostics.v1",
        }
    }
}

/// Snapshot of one independently addressable part of a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndpointSnapshot {
    /// Stable adapter-owned endpoint identifier.
    pub id: EndpointId,
    /// Optional device-provided display name.
    pub name: Option<String>,
    /// Current normalized capabilities.
    pub capabilities: Vec<CapabilitySnapshot>,
}

/// Network location observed during discovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkLocation {
    /// Host or IP address.
    pub host: String,
    /// Service port.
    pub port: u16,
}

/// Current adapter-projected view of a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    /// Stable `HomeMagic` device identifier.
    pub id: DeviceId,
    /// Immutable native identity within the integration.
    pub native_id: String,
    /// Integration that owns the device.
    pub integration: String,
    /// Mutable display name.
    pub name: String,
    /// Device manufacturer.
    pub manufacturer: String,
    /// Manufacturer model identifier.
    pub model: String,
    /// Observed network locations.
    pub network: Vec<NetworkLocation>,
    /// Addressable parts and normalized behaviors.
    pub endpoints: Vec<EndpointSnapshot>,
    /// Time of the latest successful observation.
    pub observed_at: DateTime<Utc>,
    /// Namespaced adapter data retained for diagnostics.
    pub vendor_data: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_should_be_stable_for_native_identity() {
        let first = DeviceId::from_native("shelly", "shellyplus1pm-aabbcc");
        let second = DeviceId::from_native("shelly", "shellyplus1pm-aabbcc");

        assert_eq!(first, second);
    }

    #[test]
    fn device_id_should_be_namespaced_by_integration() {
        let shelly = DeviceId::from_native("shelly", "aabbcc");
        let matter = DeviceId::from_native("matter", "aabbcc");

        assert_ne!(shelly, matter);
    }

    #[test]
    fn capability_schema_should_be_versioned() {
        let capability = CapabilitySnapshot::OnOff {
            on: true,
            risk: RiskClass::Comfort,
        };

        assert_eq!(capability.schema(), "on_off.v1");
    }
}

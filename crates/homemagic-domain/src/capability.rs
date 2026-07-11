use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// Versioned capability contract independent of display metadata and values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Stable capability name without a version suffix.
    pub name: String,
    /// Positive schema version.
    pub version: u16,
    /// Risk classification for future commands.
    pub risk: RiskClass,
}

impl CapabilityDescriptor {
    /// Creates a validated descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty name or version zero.
    pub fn new(
        name: impl Into<String>,
        version: u16,
        risk: RiskClass,
    ) -> Result<Self, CapabilityDescriptorError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(CapabilityDescriptorError::EmptyName);
        }
        if version == 0 {
            return Err(CapabilityDescriptorError::InvalidVersion);
        }
        Ok(Self {
            name,
            version,
            risk,
        })
    }

    /// Returns the transport schema identifier, such as `on_off.v1`.
    #[must_use]
    pub fn schema(&self) -> String {
        format!("{}.v{}", self.name, self.version)
    }
}

/// Invalid capability descriptor.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDescriptorError {
    /// The stable capability name was empty.
    #[error("capability name must not be empty")]
    EmptyName,
    /// Schema versions start at one.
    #[error("capability version must be greater than zero")]
    InvalidVersion,
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

    /// Returns the versioned descriptor independently from current values.
    #[must_use]
    pub fn descriptor(&self) -> CapabilityDescriptor {
        let (name, risk) = match self {
            Self::Availability { .. }
            | Self::Power { .. }
            | Self::Energy { .. }
            | Self::Diagnostics { .. } => {
                (self.schema().trim_end_matches(".v1"), RiskClass::Observe)
            }
            Self::OnOff { risk, .. } | Self::Level { risk, .. } | Self::Position { risk, .. } => {
                (self.schema().trim_end_matches(".v1"), *risk)
            }
        };
        CapabilityDescriptor {
            name: name.to_owned(),
            version: 1,
            risk,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_should_reject_version_zero() {
        let result = CapabilityDescriptor::new("on_off", 0, RiskClass::Comfort);

        assert_eq!(result, Err(CapabilityDescriptorError::InvalidVersion));
    }

    #[test]
    fn capability_schema_should_be_versioned() {
        let capability = CapabilitySnapshot::OnOff {
            on: true,
            risk: RiskClass::Comfort,
        };

        assert_eq!(capability.descriptor().schema(), "on_off.v1");
    }
}

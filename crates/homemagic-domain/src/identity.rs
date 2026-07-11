use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEVICE_NAMESPACE: Uuid = Uuid::from_u128(0x91d0_41aa_328c_5ba1_aaf6_e116_81a1_0cc9);
const INTEGRATION_NAMESPACE: Uuid = Uuid::from_u128(0xa75d_dbe0_0bd8_5ed4_9ff2_3af9_a4d6_eb65);
const LEGACY_INSTALLATION: Uuid = Uuid::from_u128(0xc776_218d_d377_5a5e_b6a7_9384_dc1c_da37);

/// Stable opaque identifier for a `HomeMagic` installation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallationId(Uuid);

impl InstallationId {
    /// Generates an installation identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for InstallationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for InstallationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for InstallationId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// Stable identity for one configured integration instance.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntegrationId(Uuid);

impl IntegrationId {
    /// Derives an integration identity from its installation, adapter, and
    /// immutable instance key.
    #[must_use]
    pub fn from_native(
        installation_id: &InstallationId,
        adapter: &str,
        instance_key: &str,
    ) -> Self {
        let key = format!("{installation_id}:{adapter}:{instance_key}");
        Self(Uuid::new_v5(&INTEGRATION_NAMESPACE, key.as_bytes()))
    }

    pub(crate) fn legacy(adapter: &str) -> Self {
        let installation = InstallationId(LEGACY_INSTALLATION);
        Self::from_native(&installation, adapter, "default")
    }
}

impl fmt::Display for IntegrationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for IntegrationId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// Stable opaque identifier for a device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(Uuid);

impl DeviceId {
    /// Derives a stable device ID inside an integration instance.
    ///
    /// ```
    /// use homemagic_domain::{DeviceId, InstallationId, IntegrationId};
    ///
    /// let installation = InstallationId::new();
    /// let integration = IntegrationId::from_native(&installation, "shelly", "local");
    /// let first = DeviceId::from_integration(&integration, "shellyplus1-aabbcc");
    /// let second = DeviceId::from_integration(&integration, "shellyplus1-aabbcc");
    /// assert_eq!(first, second);
    /// ```
    #[must_use]
    pub fn from_integration(integration_id: &IntegrationId, native_id: &str) -> Self {
        let key = format!("{integration_id}:{native_id}");
        Self(Uuid::new_v5(&DEVICE_NAMESPACE, key.as_bytes()))
    }

    /// Derives an ID for the prototype's single integration instance.
    ///
    /// Durable runtime code should use [`Self::from_integration`] so multiple
    /// instances of one adapter cannot collide.
    #[must_use]
    pub fn from_native(integration: &str, native_id: &str) -> Self {
        Self::from_integration(&IntegrationId::legacy(integration), native_id)
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

/// Stable adapter identity for an independently addressable endpoint.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EndpointId(String);

impl EndpointId {
    /// Creates an endpoint ID from an adapter-owned stable component key.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the adapter-owned component key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable opaque identifier for a semantic space.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpaceId(Uuid);

impl SpaceId {
    /// Generates a space identifier independent of its mutable name.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SpaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for an immutable domain event.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(Uuid);

impl EventId {
    /// Generates an event identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identifier for a repair record.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepairId(Uuid);

impl RepairId {
    /// Generates a repair identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RepairId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RepairId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Identifier shared by causally related operations and events.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    /// Generates a correlation identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_should_be_stable_inside_integration_instance() {
        let installation = InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "shelly", "local");

        let first = DeviceId::from_integration(&integration, "aabbcc");
        let second = DeviceId::from_integration(&integration, "aabbcc");

        assert_eq!(first, second);
    }

    #[test]
    fn device_id_should_be_namespaced_by_integration_instance() {
        let installation = InstallationId::new();
        let first = IntegrationId::from_native(&installation, "shelly", "first");
        let second = IntegrationId::from_native(&installation, "shelly", "second");

        assert_ne!(
            DeviceId::from_integration(&first, "aabbcc"),
            DeviceId::from_integration(&second, "aabbcc")
        );
    }
}

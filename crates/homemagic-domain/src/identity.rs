use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEVICE_NAMESPACE: Uuid = Uuid::from_u128(0x91d0_41aa_328c_5ba1_aaf6_e116_81a1_0cc9);
const INTEGRATION_NAMESPACE: Uuid = Uuid::from_u128(0xa75d_dbe0_0bd8_5ed4_9ff2_3af9_a4d6_eb65);
const REPAIR_NAMESPACE: Uuid = Uuid::from_u128(0x36bf_9702_60d4_5a68_aaf7_8a85_276b_693b);
const AUTOMATION_TRACE_NAMESPACE: Uuid = Uuid::from_u128(0x6af5_981c_1317_5fb7_aa73_7a66_7c98_7fc1);
const AUTOMATION_OCCURRENCE_NAMESPACE: Uuid =
    Uuid::from_u128(0x0797_c134_2a95_5872_9f84_ef69_0fb6_b8aa);
const AUTOMATION_RUN_NAMESPACE: Uuid = Uuid::from_u128(0x4956_eaca_1fa9_597d_9219_77cb_d73e_eaf5);
const AUTOMATION_TIMER_NAMESPACE: Uuid = Uuid::from_u128(0x6a52_b568_0174_5c08_9f45_b9c0_ee6d_8bc5);
const CORRELATION_NAMESPACE: Uuid = Uuid::from_u128(0xb5b5_5d91_a4a0_5411_afb2_9ec2_d254_6032);
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

/// Opaque reference to secret material owned by a configured secret backend.
///
/// The value identifies a secret; it never contains the secret itself.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a random reference suitable for a new secret.
    #[must_use]
    pub fn new() -> Self {
        Self(format!("secret-{}", Uuid::new_v4()))
    }

    /// Creates a reference from a backend-owned stable identifier.
    #[must_use]
    pub fn from_backend_id(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the non-secret backend identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SecretRef {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

macro_rules! uuid_identity {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new opaque identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_identity!(ActorId, "Stable identity for an authenticated actor.");
uuid_identity!(CommandId, "Stable identity for one durable command.");
uuid_identity!(GrantId, "Stable identity for one actor policy grant.");
uuid_identity!(AuditId, "Stable identity for one immutable audit record.");
uuid_identity!(AutomationId, "Stable identity for one automation.");
uuid_identity!(AutomationRunId, "Stable identity for one automation run.");
uuid_identity!(
    AutomationOccurrenceId,
    "Stable identity for one automation trigger occurrence."
);

impl AutomationOccurrenceId {
    /// Derives the idempotent identity of one source occurrence key.
    #[must_use]
    pub fn from_key(automation_id: &AutomationId, version: u64, source_key: &str) -> Self {
        Self(Uuid::new_v5(
            &AUTOMATION_OCCURRENCE_NAMESPACE,
            format!("{automation_id}:{version}:{source_key}").as_bytes(),
        ))
    }
}
uuid_identity!(
    AutomationTimerId,
    "Stable identity for one durable automation timer."
);

impl AutomationRunId {
    /// Derives one stable run identity from an accepted occurrence.
    #[must_use]
    pub fn from_occurrence(occurrence_id: &AutomationOccurrenceId) -> Self {
        Self(Uuid::new_v5(
            &AUTOMATION_RUN_NAMESPACE,
            occurrence_id.to_string().as_bytes(),
        ))
    }
}

impl AutomationTimerId {
    /// Derives one stable timer identity from its run, node, and ready instant.
    #[must_use]
    pub fn from_key(run_id: &AutomationRunId, node_id: u32, ready_at_millis: i64) -> Self {
        Self(Uuid::new_v5(
            &AUTOMATION_TIMER_NAMESPACE,
            format!("{run_id}:{node_id}:{ready_at_millis}").as_bytes(),
        ))
    }
}
uuid_identity!(
    AutomationTraceId,
    "Stable identity for one automation trace step."
);

impl AutomationTraceId {
    /// Derives a deterministic trace identity from one run-local sequence.
    #[must_use]
    pub fn from_run_sequence(run_id: &AutomationRunId, sequence: u64) -> Self {
        Self(Uuid::new_v5(
            &AUTOMATION_TRACE_NAMESPACE,
            format!("{run_id}:{sequence}").as_bytes(),
        ))
    }
}
uuid_identity!(
    AutomationApprovalId,
    "Stable identity for one immutable automation approval decision."
);

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

impl FromStr for SpaceId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
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

    /// Derives a stable repair identity for one device condition.
    #[must_use]
    pub fn from_condition(device_id: &DeviceId, condition: &str) -> Self {
        Self(Uuid::new_v5(
            &REPAIR_NAMESPACE,
            format!("{device_id}:{condition}").as_bytes(),
        ))
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

impl FromStr for RepairId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
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

    /// Derives a deterministic correlation identity for durable work recovery.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        Self(Uuid::new_v5(&CORRELATION_NAMESPACE, key.as_bytes()))
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for CorrelationId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
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

    #[test]
    fn automation_work_ids_should_be_stable_for_restart_keys() {
        let automation_id = AutomationId::new();
        let occurrence = AutomationOccurrenceId::from_key(&automation_id, 3, "event:42");
        let repeated = AutomationOccurrenceId::from_key(&automation_id, 3, "event:42");
        let run = AutomationRunId::from_occurrence(&occurrence);

        assert_eq!(occurrence, repeated);
        assert_eq!(run, AutomationRunId::from_occurrence(&repeated));
        assert_eq!(
            AutomationTimerId::from_key(&run, 7, 1_725_000_000_000),
            AutomationTimerId::from_key(&run, 7, 1_725_000_000_000)
        );
        assert_ne!(
            AutomationTimerId::from_key(&run, 7, 1_725_000_000_000),
            AutomationTimerId::from_key(&run, 8, 1_725_000_000_000)
        );
    }
}

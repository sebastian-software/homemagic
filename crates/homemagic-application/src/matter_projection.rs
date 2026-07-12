//! Versioned Matter-to-HomeMagic capability projection and report ordering.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use homemagic_domain::{
    CapabilityDescriptor, CapabilityObservation, CausationMetadata, DeviceId, EndpointId,
    InstallationId, IntegrationId, MatterAttributePath, MatterAttributeReport,
    MatterAttributeValue, MatterClusterDescriptor, MatterConvergence, MatterDeviceType,
    MatterEndpointDescriptor, MatterFabricId, MatterLockState, MatterNodeDescriptor, MatterNodeId,
    MatterProjectedState, MatterProjectionId, MatterReportedState, MatterStateError,
    MatterStateFreshness, MatterStateUncertainty, MatterStateValue, MatterSubscriptionId,
    ObservationSource, ObservationSourceKind, ObservedValue, RiskClass,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::StoredMatterProjection;

/// Matter On/Off cluster.
pub const MATTER_ON_OFF_CLUSTER_ID: u32 = 0x0006;
/// Matter Level Control cluster, reserved for a later fixture-backed rule.
pub const MATTER_LEVEL_CONTROL_CLUSTER_ID: u32 = 0x0008;
/// Matter Door Lock cluster.
pub const MATTER_DOOR_LOCK_CLUSTER_ID: u32 = 0x0101;
/// Matter Window Covering cluster, reserved for a later fixture-backed rule.
pub const MATTER_WINDOW_COVERING_CLUSTER_ID: u32 = 0x0102;

const ON_OFF_ATTRIBUTE_ID: u32 = 0x0000;
const LOCK_STATE_ATTRIBUTE_ID: u32 = 0x0000;
const OFF_COMMAND_ID: u32 = 0x0000;
const ON_COMMAND_ID: u32 = 0x0001;
const LOCK_COMMAND_ID: u32 = 0x0000;
const UNLOCK_COMMAND_ID: u32 = 0x0001;
const ON_OFF_LIGHT_DEVICE_TYPE: u32 = 0x0100;
const DIMMABLE_LIGHT_DEVICE_TYPE: u32 = 0x0101;
const DOOR_LOCK_DEVICE_TYPE: u32 = 0x000A;

/// One explicit projection rule known to the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterProjectionRule {
    /// Applicable On/Off server semantics.
    OnOffV1,
    /// Applicable Door Lock server semantics.
    AccessControlV1,
    /// Declared but disabled until a fixture proves Level Control semantics.
    LevelV1LaterFixture,
    /// Declared but disabled until calibration and motion semantics are proven.
    PositionV1LaterFixture,
}

impl MatterProjectionRule {
    /// Returns the common schema emitted by an enabled rule.
    #[must_use]
    pub const fn capability_schema(self) -> Option<&'static str> {
        match self {
            Self::OnOffV1 => Some("on_off.v1"),
            Self::AccessControlV1 => Some("access_control.v1"),
            Self::LevelV1LaterFixture | Self::PositionV1LaterFixture => None,
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::OnOffV1 => "on_off",
            Self::AccessControlV1 => "access_control",
            Self::LevelV1LaterFixture => "level",
            Self::PositionV1LaterFixture => "position",
        }
    }

    const VERSION: u16 = 1;
}

/// Read-only cluster metadata that did not become a common capability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatterDiagnosticProjection {
    /// Versioned namespace; deliberately not a public cluster command surface.
    pub namespace: String,
    /// Whether the endpoint exposes the cluster as a server or client.
    pub role: MatterClusterRole,
    /// Cluster revision used for future semantic review.
    pub revision: u16,
    /// Feature bits retained as diagnostics only.
    pub feature_map: u32,
    /// Bounded available attributes.
    pub attributes: Vec<u32>,
    /// Bounded accepted commands. These are diagnostic identifiers, not invocable APIs.
    pub accepted_commands: Vec<u32>,
}

/// Descriptor role for namespaced diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterClusterRole {
    /// Endpoint implements cluster behavior.
    Server,
    /// Endpoint consumes cluster behavior.
    Client,
}

/// One stable common capability projected from a Matter endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterCapabilityProjection {
    /// Owning integration instance, stable for one fabric.
    pub integration_id: IntegrationId,
    /// Stable common device identity.
    pub device_id: DeviceId,
    /// Stable endpoint identity within the device.
    pub endpoint_id: EndpointId,
    /// Stable projection identity, independent from descriptor revisions.
    pub projection_id: MatterProjectionId,
    /// Applied rule.
    pub rule: MatterProjectionRule,
    /// Common capability contract.
    pub capability: CapabilityDescriptor,
    /// Only report path admitted by this initial scalar rule.
    pub report_path: MatterAttributePath,
    /// Descriptor revision on which command assumptions depend.
    pub projection_revision: u64,
    /// Source cluster revision.
    pub cluster_revision: u16,
    /// Source feature map.
    pub feature_map: u32,
}

/// Complete bounded projection result for one node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterNodeProjection {
    /// Stable common device identity.
    pub device_id: DeviceId,
    /// Enabled common capabilities only.
    pub capabilities: Vec<MatterCapabilityProjection>,
    /// Unmapped protocol metadata retained read-only.
    pub diagnostics: Vec<MatterDiagnosticProjection>,
}

impl MatterNodeProjection {
    /// Locates the projection accepting a report path.
    #[must_use]
    pub fn projection_for(&self, path: MatterAttributePath) -> Option<&MatterCapabilityProjection> {
        self.capabilities
            .iter()
            .find(|projection| projection.report_path == path)
    }
}

/// Projects one bounded descriptor through the accepted v1 rules.
#[must_use]
pub fn project_matter_node(
    installation_id: &InstallationId,
    descriptor: &MatterNodeDescriptor,
) -> MatterNodeProjection {
    let integration_id = IntegrationId::from_native(
        installation_id,
        "matter",
        &descriptor.fabric_id().to_string(),
    );
    let device_id = DeviceId::from_integration(
        &integration_id,
        &format!("node:{}", descriptor.node_id().get()),
    );
    let mut capabilities = Vec::new();
    let mut mapped_clusters = BTreeSet::new();

    for endpoint in descriptor.endpoints() {
        if let Some(projection) = project_on_off(&integration_id, &device_id, descriptor, endpoint)
        {
            mapped_clusters.insert((endpoint.number().get(), MATTER_ON_OFF_CLUSTER_ID));
            capabilities.push(projection);
        }
        if let Some(projection) =
            project_access_control(&integration_id, &device_id, descriptor, endpoint)
        {
            mapped_clusters.insert((endpoint.number().get(), MATTER_DOOR_LOCK_CLUSTER_ID));
            capabilities.push(projection);
        }
    }

    let diagnostics = descriptor
        .endpoints()
        .iter()
        .flat_map(|endpoint| {
            let server = endpoint
                .server_clusters()
                .iter()
                .filter(|cluster| {
                    !mapped_clusters.contains(&(endpoint.number().get(), cluster.id()))
                })
                .map(|cluster| diagnostic(endpoint, cluster, MatterClusterRole::Server));
            let client = endpoint
                .client_clusters()
                .iter()
                .map(|cluster| diagnostic(endpoint, cluster, MatterClusterRole::Client));
            server.chain(client)
        })
        .collect();

    MatterNodeProjection {
        device_id,
        capabilities,
        diagnostics,
    }
}

fn project_on_off(
    integration_id: &IntegrationId,
    device_id: &DeviceId,
    node: &MatterNodeDescriptor,
    endpoint: &MatterEndpointDescriptor,
) -> Option<MatterCapabilityProjection> {
    if !has_device_type(
        endpoint.device_types(),
        &[ON_OFF_LIGHT_DEVICE_TYPE, DIMMABLE_LIGHT_DEVICE_TYPE],
    ) {
        return None;
    }
    let cluster = eligible_server_cluster(
        endpoint,
        MATTER_ON_OFF_CLUSTER_ID,
        &[ON_OFF_ATTRIBUTE_ID],
        &[OFF_COMMAND_ID, ON_COMMAND_ID],
    )?;
    projection(
        integration_id,
        device_id,
        node,
        endpoint,
        cluster,
        MatterProjectionRule::OnOffV1,
        ON_OFF_ATTRIBUTE_ID,
        RiskClass::Comfort,
    )
}

fn project_access_control(
    integration_id: &IntegrationId,
    device_id: &DeviceId,
    node: &MatterNodeDescriptor,
    endpoint: &MatterEndpointDescriptor,
) -> Option<MatterCapabilityProjection> {
    if !has_device_type(endpoint.device_types(), &[DOOR_LOCK_DEVICE_TYPE]) {
        return None;
    }
    let cluster = eligible_server_cluster(
        endpoint,
        MATTER_DOOR_LOCK_CLUSTER_ID,
        &[LOCK_STATE_ATTRIBUTE_ID],
        &[LOCK_COMMAND_ID, UNLOCK_COMMAND_ID],
    )?;
    projection(
        integration_id,
        device_id,
        node,
        endpoint,
        cluster,
        MatterProjectionRule::AccessControlV1,
        LOCK_STATE_ATTRIBUTE_ID,
        RiskClass::Security,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "all rule assumptions remain explicit at the projection boundary"
)]
fn projection(
    integration_id: &IntegrationId,
    device_id: &DeviceId,
    node: &MatterNodeDescriptor,
    endpoint: &MatterEndpointDescriptor,
    cluster: &MatterClusterDescriptor,
    rule: MatterProjectionRule,
    attribute_id: u32,
    risk: RiskClass,
) -> Option<MatterCapabilityProjection> {
    let schema = rule.capability_schema()?;
    let name = schema.strip_suffix(".v1")?;
    Some(MatterCapabilityProjection {
        integration_id: integration_id.clone(),
        device_id: device_id.clone(),
        endpoint_id: EndpointId::new(format!("matter:{}", endpoint.number().get())),
        projection_id: MatterProjectionId::from_key(
            node.fabric_id(),
            node.node_id().get(),
            endpoint.number().get(),
            rule.key(),
            MatterProjectionRule::VERSION,
        ),
        rule,
        capability: CapabilityDescriptor::new(name, MatterProjectionRule::VERSION, risk).ok()?,
        report_path: MatterAttributePath {
            node_id: node.node_id(),
            endpoint: endpoint.number(),
            cluster_id: cluster.id(),
            attribute_id,
        },
        projection_revision: node.descriptor_revision().get(),
        cluster_revision: cluster.revision(),
        feature_map: cluster.feature_map(),
    })
}

fn has_device_type(device_types: &[MatterDeviceType], eligible: &[u32]) -> bool {
    device_types
        .iter()
        .any(|device_type| eligible.contains(&device_type.id))
}

fn eligible_server_cluster<'a>(
    endpoint: &'a MatterEndpointDescriptor,
    cluster_id: u32,
    attributes: &[u32],
    commands: &[u32],
) -> Option<&'a MatterClusterDescriptor> {
    endpoint.server_clusters().iter().find(|cluster| {
        cluster.id() == cluster_id
            && attributes
                .iter()
                .all(|attribute| cluster.attributes().contains(attribute))
            && commands
                .iter()
                .all(|command| cluster.accepted_commands().contains(command))
    })
}

fn diagnostic(
    endpoint: &MatterEndpointDescriptor,
    cluster: &MatterClusterDescriptor,
    role: MatterClusterRole,
) -> MatterDiagnosticProjection {
    MatterDiagnosticProjection {
        namespace: format!(
            "matter.diagnostics.v1.endpoint.{}.cluster.{:08x}",
            endpoint.number().get(),
            cluster.id()
        ),
        role,
        revision: cluster.revision(),
        feature_map: cluster.feature_map(),
        attributes: cluster.attributes().to_vec(),
        accepted_commands: cluster.accepted_commands().to_vec(),
    }
}

/// Result of comparing durable projection assumptions with rediscovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatterProjectionValidity {
    /// Rule inputs are unchanged and commands may use the cached projection.
    Current,
    /// Descriptor, cluster revision, feature map, or support changed.
    Invalidated,
}

/// Checks whether a cached projection remains command-safe after rediscovery.
#[must_use]
pub fn projection_validity(
    cached: &MatterCapabilityProjection,
    rediscovered: &MatterNodeProjection,
) -> MatterProjectionValidity {
    rediscovered
        .capabilities
        .iter()
        .find(|candidate| candidate.projection_id == cached.projection_id)
        .filter(|candidate| {
            candidate.report_path == cached.report_path
                && candidate.projection_revision == cached.projection_revision
                && candidate.cluster_revision == cached.cluster_revision
                && candidate.feature_map == cached.feature_map
                && candidate.capability == cached.capability
        })
        .map_or(MatterProjectionValidity::Invalidated, |_| {
            MatterProjectionValidity::Current
        })
}

/// Creates the first durable projection row with explicit unknown freshness.
///
/// # Errors
///
/// Returns a domain consistency error if the initial state cannot be formed.
pub fn initial_stored_matter_projection(
    installation_id: InstallationId,
    fabric_id: MatterFabricId,
    projection: &MatterCapabilityProjection,
    now: DateTime<Utc>,
) -> Result<StoredMatterProjection, MatterStateError> {
    let state = MatterProjectedState::new(
        projection.projection_id.clone(),
        None,
        None,
        None,
        MatterStateFreshness::Unknown,
        MatterConvergence::NoDesiredState,
        None,
    )?;
    Ok(StoredMatterProjection {
        installation_id,
        fabric_id,
        node_id: projection.report_path.node_id,
        endpoint_number: projection.report_path.endpoint,
        projection_id: projection.projection_id.clone(),
        device_id: projection.device_id.clone(),
        endpoint_id: projection.endpoint_id.clone(),
        capability_schema: projection.capability.schema(),
        projection_revision: projection.projection_revision,
        state,
        revision: 1,
        updated_at: now,
    })
}

/// Causation retained while converting a protocol report to common observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatterReportCausation {
    /// Shared common-domain causation, when the report follows known work.
    pub common: Option<CausationMetadata>,
    /// Desired revision that may be confirmed by this report.
    pub desired_revision: Option<u64>,
}

/// Accepted, duplicate, or rejected report outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum MatterReportDecision {
    /// Report advanced trusted state.
    Applied {
        /// Durable Matter-specific reported state.
        reported: MatterReportedState,
        /// Common capability observation.
        observation: Box<CapabilityObservation>,
        /// Explicit causal context.
        causation: MatterReportCausation,
    },
    /// Byte-equivalent or semantically equivalent report was already applied.
    Duplicate,
    /// Report cannot safely move state forward.
    Rejected(MatterReportRejection),
}

/// Stable reason a report did not move projected state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum MatterReportRejection {
    /// Report targets a different projection path.
    #[error("report path does not match projection")]
    TargetMismatch,
    /// Report sequence is older than the durable state.
    #[error("report sequence is stale")]
    StaleSequence,
    /// Equal sequence carried different content.
    #[error("duplicate report sequence conflicts with durable state")]
    ConflictingDuplicate,
    /// Data version moved backwards or changed value without advancing.
    #[error("report data version is stale or conflicting")]
    StaleDataVersion,
    /// Scalar cannot be represented by the selected common capability.
    #[error("report value is incompatible with projection")]
    IncompatibleValue,
    /// Source time is after receive time.
    #[error("report time is invalid")]
    InvalidTime,
}

/// Normalizes one scalar report with durable ordering and deduplication.
#[must_use]
pub fn normalize_matter_report(
    projection: &MatterCapabilityProjection,
    report: &MatterAttributeReport,
    received_at: DateTime<Utc>,
    current: Option<&MatterReportedState>,
    source_kind: ObservationSourceKind,
    causation: MatterReportCausation,
) -> MatterReportDecision {
    if report.path != projection.report_path {
        return MatterReportDecision::Rejected(MatterReportRejection::TargetMismatch);
    }
    let Some((state_value, values)) =
        map_report_value(projection.rule, &report.value, report.observed_at)
    else {
        return MatterReportDecision::Rejected(MatterReportRejection::IncompatibleValue);
    };
    if report.observed_at > received_at {
        return MatterReportDecision::Rejected(MatterReportRejection::InvalidTime);
    }
    if let Some(current) = current {
        if report.report_sequence < current.report_sequence() {
            return MatterReportDecision::Rejected(MatterReportRejection::StaleSequence);
        }
        if report.report_sequence == current.report_sequence() {
            return if current.value() == &state_value
                && current.data_version() == report.data_version
            {
                MatterReportDecision::Duplicate
            } else {
                MatterReportDecision::Rejected(MatterReportRejection::ConflictingDuplicate)
            };
        }
        if !data_version_advances(current, report, &state_value) {
            return MatterReportDecision::Rejected(MatterReportRejection::StaleDataVersion);
        }
    }
    let Ok(reported) = MatterReportedState::new(
        state_value,
        report.data_version,
        report.report_sequence,
        report.observed_at,
        received_at,
    ) else {
        return MatterReportDecision::Rejected(MatterReportRejection::InvalidTime);
    };
    MatterReportDecision::Applied {
        reported,
        observation: Box::new(CapabilityObservation {
            device_id: projection.device_id.clone(),
            endpoint_id: projection.endpoint_id.clone(),
            capability: projection.capability.clone(),
            values,
            received_at,
            source: ObservationSource {
                integration_id: projection.integration_id.clone(),
                kind: source_kind,
                sequence: Some(report.report_sequence),
            },
        }),
        causation,
    }
}

fn data_version_advances(
    current: &MatterReportedState,
    report: &MatterAttributeReport,
    next_value: &MatterStateValue,
) -> bool {
    match (current.data_version(), report.data_version) {
        (Some(previous), Some(next)) if previous == next => current.value() == next_value,
        (Some(previous), Some(next)) => next.wrapping_sub(previous) < (1_u32 << 31),
        _ => true,
    }
}

fn map_report_value(
    rule: MatterProjectionRule,
    value: &MatterAttributeValue,
    observed_at: DateTime<Utc>,
) -> Option<(MatterStateValue, BTreeMap<String, ObservedValue>)> {
    match (rule, value) {
        (MatterProjectionRule::OnOffV1, MatterAttributeValue::Boolean(on)) => Some((
            MatterStateValue::OnOff(*on),
            BTreeMap::from([(
                "on".to_owned(),
                ObservedValue {
                    value: json!(on),
                    observed_at,
                },
            )]),
        )),
        (MatterProjectionRule::AccessControlV1, MatterAttributeValue::Unsigned(raw)) => {
            let state = match raw {
                1 => MatterLockState::NotFullyLocked,
                2 => MatterLockState::Unlocked,
                3 => MatterLockState::Locked,
                _ => MatterLockState::Unknown,
            };
            let locked = match state {
                MatterLockState::Locked => Some(true),
                MatterLockState::Unlocked => Some(false),
                MatterLockState::NotFullyLocked | MatterLockState::Unknown => None,
            };
            Some((
                MatterStateValue::Lock(state),
                BTreeMap::from([
                    (
                        "state".to_owned(),
                        ObservedValue {
                            value: json!(match state {
                                MatterLockState::Locked => "locked",
                                MatterLockState::Unlocked => "unlocked",
                                MatterLockState::NotFullyLocked => "not_fully_locked",
                                MatterLockState::Unknown => "unknown",
                            }),
                            observed_at,
                        },
                    ),
                    (
                        "locked".to_owned(),
                        ObservedValue {
                            value: json!(locked),
                            observed_at,
                        },
                    ),
                ]),
            ))
        }
        _ => None,
    }
}

/// Creates explicit stale state immediately after subscription or descriptor loss.
///
/// # Errors
///
/// Returns a domain consistency error if the supplied state was already invalid.
pub fn mark_matter_projection_stale(
    state: &MatterProjectedState,
    uncertainty: MatterStateUncertainty,
) -> Result<MatterProjectedState, MatterStateError> {
    MatterProjectedState::new(
        state.projection_id().clone(),
        state.desired().cloned(),
        state.reported().cloned(),
        state.confirmed_revision(),
        MatterStateFreshness::Stale,
        match state.convergence() {
            MatterConvergence::Confirmed if state.desired().is_some() => MatterConvergence::Pending,
            convergence => convergence,
        },
        Some(uncertainty),
    )
}

/// Advances durable projected state after an accepted report.
///
/// Confirmation requires both a matching value and the exact desired revision
/// retained as report causation. A matching value without that binding remains
/// pending because it may predate the command.
///
/// # Errors
///
/// Returns a domain consistency error if the resulting state is invalid.
pub fn advance_matter_projected_state(
    state: &MatterProjectedState,
    reported: MatterReportedState,
    causation: &MatterReportCausation,
) -> Result<MatterProjectedState, MatterStateError> {
    let (confirmed_revision, convergence) = match state.desired() {
        None => (None, MatterConvergence::NoDesiredState),
        Some(desired)
            if desired.value == *reported.value()
                && causation.desired_revision == Some(desired.revision.get()) =>
        {
            (Some(desired.revision), MatterConvergence::Confirmed)
        }
        Some(desired) if desired.value == *reported.value() => {
            (state.confirmed_revision(), MatterConvergence::Pending)
        }
        Some(_) => (state.confirmed_revision(), MatterConvergence::Diverged),
    };
    MatterProjectedState::new(
        state.projection_id().clone(),
        state.desired().cloned(),
        Some(reported),
        confirmed_revision,
        MatterStateFreshness::Fresh,
        convergence,
        None,
    )
}

/// Replaces the latest desired revision while preserving reported evidence.
///
/// # Errors
///
/// Returns a domain consistency error if the resulting projection is invalid.
pub fn advance_matter_desired_state(
    state: &MatterProjectedState,
    desired: homemagic_domain::MatterDesiredState,
) -> Result<MatterProjectedState, MatterStateError> {
    MatterProjectedState::new(
        state.projection_id().clone(),
        Some(desired),
        state.reported().cloned(),
        state.confirmed_revision(),
        state.freshness(),
        MatterConvergence::Pending,
        state.uncertainty(),
    )
}

/// Stable node-level subscription identity used across process restarts.
#[must_use]
pub fn matter_subscription_id(
    fabric_id: &MatterFabricId,
    node_id: MatterNodeId,
) -> MatterSubscriptionId {
    MatterSubscriptionId::from_node(fabric_id, node_id.get())
}

//! Contract evidence for Matter capability projection and subscription recovery.

use std::error::Error;
use std::str::FromStr;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    MatterAttributeSelection, MatterProjectionRule, MatterProjectionValidity,
    MatterReportCausation, MatterReportDecision, MatterReportRejection, MatterSubscriptionRecovery,
    MatterSubscriptionRecoveryAction, MatterSubscriptionRecoveryOutcome,
    MatterSubscriptionRecoveryPolicy, StoredMatterSubscription, StoredMatterSubscriptionState,
    advance_matter_projected_state, initial_stored_matter_projection, matter_subscription_id,
    normalize_matter_report, project_matter_node, projection_validity,
};
use homemagic_domain::{
    InstallationId, MatterAttributePath, MatterAttributeReport, MatterAttributeValue,
    MatterClusterDescriptor, MatterConvergence, MatterDescriptorRevision, MatterDesiredState,
    MatterDeviceType, MatterEndpointDescriptor, MatterEndpointNumber, MatterFabricId,
    MatterNodeDescriptor, MatterNodeId, MatterProjectedState, MatterProjectionId,
    MatterReportedState, MatterStateFreshness, MatterStateRevision, MatterStateValue,
    MatterSubscriptionLossReason, ObservationSourceKind,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

const LIGHT_DEVICE_TYPE: u32 = 0x0100;
const LOCK_DEVICE_TYPE: u32 = 0x000A;
const ON_OFF_CLUSTER: u32 = 0x0006;
const DOOR_LOCK_CLUSTER: u32 = 0x0101;

fn now() -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")?.with_timezone(&Utc))
}

fn installation_id() -> Result<InstallationId, uuid::Error> {
    InstallationId::from_str("00000000-0000-4000-8000-000000000001")
}

fn fabric_id() -> Result<MatterFabricId, uuid::Error> {
    MatterFabricId::from_str("10000000-0000-4000-8000-000000000001")
}

fn descriptor(
    revision: u64,
    device_type: u32,
    server: bool,
    cluster_id: u32,
    feature_map: u32,
    attributes: Vec<u32>,
    commands: Vec<u32>,
) -> Result<MatterNodeDescriptor, Box<dyn Error + Send + Sync>> {
    let cluster =
        MatterClusterDescriptor::with_attributes(cluster_id, 1, feature_map, commands, attributes)?;
    let (servers, clients) = if server {
        (vec![cluster], Vec::new())
    } else {
        (Vec::new(), vec![cluster])
    };
    Ok(MatterNodeDescriptor::new(
        fabric_id()?,
        MatterNodeId::new(42)?,
        vec![MatterEndpointDescriptor::new(
            MatterEndpointNumber::new(1),
            vec![MatterDeviceType::new(device_type, 1)?],
            servers,
            clients,
        )?],
        MatterDescriptorRevision::new(revision)?,
    )?)
}

fn light_descriptor(revision: u64) -> Result<MatterNodeDescriptor, Box<dyn Error + Send + Sync>> {
    descriptor(
        revision,
        LIGHT_DEVICE_TYPE,
        true,
        ON_OFF_CLUSTER,
        0,
        vec![0],
        vec![0, 1],
    )
}

#[test]
fn projection_matrix_should_require_device_type_server_role_attribute_and_commands() -> TestResult {
    let cases = [
        (LIGHT_DEVICE_TYPE, true, vec![0], vec![0, 1], true),
        (LIGHT_DEVICE_TYPE, false, vec![0], vec![0, 1], false),
        (0x0016, true, vec![0], vec![0, 1], false),
        (LIGHT_DEVICE_TYPE, true, Vec::new(), vec![0, 1], false),
        (LIGHT_DEVICE_TYPE, true, vec![0], vec![1], false),
    ];

    for (device_type, server, attributes, commands, projected) in cases {
        let descriptor = descriptor(
            1,
            device_type,
            server,
            ON_OFF_CLUSTER,
            0,
            attributes,
            commands,
        )?;
        let projection = project_matter_node(&installation_id()?, &descriptor);
        assert_eq!(
            projection.capabilities.len(),
            usize::from(projected),
            "unexpected projection for device type {device_type:#x}, server={server}"
        );
    }
    Ok(())
}

#[test]
fn light_and_lock_should_project_only_common_capabilities() -> TestResult {
    let light = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let lock_descriptor = descriptor(
        1,
        LOCK_DEVICE_TYPE,
        true,
        DOOR_LOCK_CLUSTER,
        0,
        vec![0],
        vec![0, 1],
    )?;
    let lock = project_matter_node(&installation_id()?, &lock_descriptor);

    assert_eq!(light.capabilities[0].capability.schema(), "on_off.v1");
    assert_eq!(light.capabilities[0].rule, MatterProjectionRule::OnOffV1);
    assert_eq!(
        lock.capabilities[0].capability.schema(),
        "access_control.v1"
    );
    assert_eq!(
        lock.capabilities[0].rule,
        MatterProjectionRule::AccessControlV1
    );
    assert!(
        serde_json::to_string(&light.diagnostics)?
            .find("write")
            .is_none(),
        "diagnostics must not create a raw-write surface"
    );
    Ok(())
}

#[test]
fn unsupported_clusters_should_remain_namespaced_read_only_diagnostics() -> TestResult {
    let descriptor = descriptor(1, LIGHT_DEVICE_TYPE, true, 0xFC01, 7, vec![1, 2], vec![3])?;
    let projection = project_matter_node(&installation_id()?, &descriptor);

    assert!(projection.capabilities.is_empty());
    assert_eq!(projection.diagnostics.len(), 1);
    assert_eq!(
        projection.diagnostics[0].namespace,
        "matter.diagnostics.v1.endpoint.1.cluster.0000fc01"
    );
    Ok(())
}

#[test]
fn later_level_and_cover_rules_should_not_activate_from_cluster_presence() -> TestResult {
    for cluster_id in [0x0008, 0x0102] {
        let descriptor = descriptor(
            1,
            LIGHT_DEVICE_TYPE,
            true,
            cluster_id,
            u32::MAX,
            vec![0, 1, 2],
            vec![0, 1, 2],
        )?;
        let projection = project_matter_node(&installation_id()?, &descriptor);

        assert!(projection.capabilities.is_empty());
        assert_eq!(projection.diagnostics.len(), 1);
    }
    assert_eq!(
        MatterProjectionRule::LevelV1LaterFixture.capability_schema(),
        None
    );
    assert_eq!(
        MatterProjectionRule::PositionV1LaterFixture.capability_schema(),
        None
    );
    Ok(())
}

#[test]
fn stable_ids_should_survive_restart_but_descriptor_revision_should_invalidate_assumptions()
-> TestResult {
    let first = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let restarted = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let changed = project_matter_node(&installation_id()?, &light_descriptor(2)?);

    assert_eq!(first.device_id, restarted.device_id);
    assert_eq!(
        first.capabilities[0].endpoint_id,
        restarted.capabilities[0].endpoint_id
    );
    assert_eq!(
        first.capabilities[0].projection_id,
        changed.capabilities[0].projection_id
    );
    assert_eq!(
        projection_validity(&first.capabilities[0], &restarted),
        MatterProjectionValidity::Current
    );
    assert_eq!(
        projection_validity(&first.capabilities[0], &changed),
        MatterProjectionValidity::Invalidated
    );
    Ok(())
}

#[test]
fn durable_projection_should_store_rule_revision_without_changing_identity() -> TestResult {
    let first = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let changed = project_matter_node(&installation_id()?, &light_descriptor(2)?);
    let first_row = initial_stored_matter_projection(
        installation_id()?,
        fabric_id()?,
        &first.capabilities[0],
        now()?,
    )?;
    let changed_row = initial_stored_matter_projection(
        installation_id()?,
        fabric_id()?,
        &changed.capabilities[0],
        now()?,
    )?;

    assert_eq!(first_row.projection_id, changed_row.projection_id);
    assert_eq!(first_row.projection_revision, 1);
    assert_eq!(changed_row.projection_revision, 2);
    assert_eq!(first_row.capability_schema, "on_off.v1");
    assert_eq!(first_row.state.freshness(), MatterStateFreshness::Unknown);
    Ok(())
}

fn report(
    sequence: u64,
    data_version: u32,
    on: bool,
) -> Result<MatterAttributeReport, Box<dyn Error + Send + Sync>> {
    Ok(MatterAttributeReport {
        path: MatterAttributePath {
            node_id: MatterNodeId::new(42)?,
            endpoint: MatterEndpointNumber::new(1),
            cluster_id: ON_OFF_CLUSTER,
            attribute_id: 0,
        },
        value: MatterAttributeValue::Boolean(on),
        data_version: Some(data_version),
        report_sequence: sequence,
        observed_at: now()?,
    })
}

fn applied_state(
    decision: MatterReportDecision,
) -> Result<MatterReportedState, Box<dyn Error + Send + Sync>> {
    match decision {
        MatterReportDecision::Applied { reported, .. } => Ok(reported),
        other => Err(format!("expected applied report, got {other:?}").into()),
    }
}

#[test]
fn reports_should_apply_once_and_reject_stale_reordered_or_conflicting_updates() -> TestResult {
    let projection = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let projection = &projection.capabilities[0];
    let causation = MatterReportCausation {
        common: None,
        desired_revision: Some(7),
    };
    let first_report = report(10, 20, true)?;
    let first = applied_state(normalize_matter_report(
        projection,
        &first_report,
        now()? + TimeDelta::milliseconds(1),
        None,
        ObservationSourceKind::Notification,
        causation.clone(),
    ))?;

    assert_eq!(
        normalize_matter_report(
            projection,
            &first_report,
            now()? + TimeDelta::milliseconds(2),
            Some(&first),
            ObservationSourceKind::Notification,
            causation.clone(),
        ),
        MatterReportDecision::Duplicate
    );
    assert_eq!(
        normalize_matter_report(
            projection,
            &report(9, 19, false)?,
            now()? + TimeDelta::milliseconds(2),
            Some(&first),
            ObservationSourceKind::Notification,
            causation.clone(),
        ),
        MatterReportDecision::Rejected(MatterReportRejection::StaleSequence)
    );
    assert_eq!(
        normalize_matter_report(
            projection,
            &report(10, 20, false)?,
            now()? + TimeDelta::milliseconds(2),
            Some(&first),
            ObservationSourceKind::Notification,
            causation.clone(),
        ),
        MatterReportDecision::Rejected(MatterReportRejection::ConflictingDuplicate)
    );
    assert_eq!(
        normalize_matter_report(
            projection,
            &report(11, 19, false)?,
            now()? + TimeDelta::milliseconds(2),
            Some(&first),
            ObservationSourceKind::Notification,
            causation,
        ),
        MatterReportDecision::Rejected(MatterReportRejection::StaleDataVersion)
    );
    Ok(())
}

#[test]
fn gap_read_should_use_refresh_provenance_and_preserve_data_version_causation() -> TestResult {
    let projection = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let causation = MatterReportCausation {
        common: None,
        desired_revision: Some(9),
    };
    let decision = normalize_matter_report(
        &projection.capabilities[0],
        &report(12, 22, false)?,
        now()? + TimeDelta::milliseconds(1),
        None,
        ObservationSourceKind::RefreshFallback,
        causation.clone(),
    );
    let MatterReportDecision::Applied {
        reported,
        observation,
        causation: actual,
    } = decision
    else {
        return Err("gap read should apply".into());
    };

    assert_eq!(reported.data_version(), Some(22));
    assert_eq!(
        observation.source.kind,
        ObservationSourceKind::RefreshFallback
    );
    assert_eq!(actual, causation);
    Ok(())
}

fn selection() -> Result<MatterAttributeSelection, Box<dyn Error + Send + Sync>> {
    Ok(MatterAttributeSelection::new(vec![
        report(1, 1, false)?.path,
    ])?)
}

fn recovery_policy() -> Result<MatterSubscriptionRecoveryPolicy, Box<dyn Error + Send + Sync>> {
    Ok(MatterSubscriptionRecoveryPolicy::new(
        2, 1, 100, 1_000, 20, 60_000,
    )?)
}

#[test]
fn subscription_loss_should_mark_stale_read_gap_and_resubscribe_in_order() -> TestResult {
    let fabric_id = fabric_id()?;
    let node_id = MatterNodeId::new(42)?;
    let mut recovery = MatterSubscriptionRecovery::after_loss(
        matter_subscription_id(&fabric_id, node_id),
        fabric_id,
        node_id,
        selection()?,
        0,
        1_000,
        MatterSubscriptionLossReason::SessionClosed,
        false,
        None,
        recovery_policy()?,
    );
    let now = now()?;

    assert!(matches!(
        recovery.next_action(now),
        MatterSubscriptionRecoveryAction::MarkStale { .. }
    ));
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::StalePersisted, now)?;
    assert!(matches!(
        recovery.next_action(now),
        MatterSubscriptionRecoveryAction::GapRead(_)
    ));
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::GapReadCompleted, now)?;
    assert!(matches!(
        recovery.next_action(now),
        MatterSubscriptionRecoveryAction::Resubscribe(_)
    ));
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::Resubscribed, now)?;
    assert_eq!(
        recovery.next_action(now),
        MatterSubscriptionRecoveryAction::Complete
    );
    Ok(())
}

#[test]
fn resubscription_should_use_bounded_retry_and_then_require_repair() -> TestResult {
    let fabric_id = fabric_id()?;
    let node_id = MatterNodeId::new(42)?;
    let mut recovery = MatterSubscriptionRecovery::after_loss(
        matter_subscription_id(&fabric_id, node_id),
        fabric_id,
        node_id,
        selection()?,
        0,
        1_000,
        MatterSubscriptionLossReason::TimedOut,
        false,
        None,
        recovery_policy()?,
    );
    let now = now()?;
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::StalePersisted, now)?;
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::GapReadFailed, now)?;
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::ResubscribeFailed, now)?;
    let MatterSubscriptionRecoveryAction::WaitUntil(retry_at) = recovery.next_action(now) else {
        return Err("first failure must wait with bounded backoff".into());
    };
    assert!(retry_at > now && retry_at <= now + TimeDelta::milliseconds(1_000));
    assert!(matches!(
        recovery.next_action(retry_at),
        MatterSubscriptionRecoveryAction::Resubscribe(_)
    ));
    recovery.record_outcome(
        MatterSubscriptionRecoveryOutcome::ResubscribeFailed,
        retry_at,
    )?;
    assert_eq!(
        recovery.next_action(retry_at),
        MatterSubscriptionRecoveryAction::RepairRequired
    );
    Ok(())
}

#[test]
fn restart_should_preserve_logical_identity_and_sleepy_device_read_bound() -> TestResult {
    let fabric_id = fabric_id()?;
    let node_id = MatterNodeId::new(42)?;
    let subscription_id = matter_subscription_id(&fabric_id, node_id);
    let stored = StoredMatterSubscription {
        subscription_id: subscription_id.clone(),
        fabric_id: fabric_id.clone(),
        node_id,
        state: StoredMatterSubscriptionState::Established,
        report_sequence: 12,
        stale_after: now()?,
        revision: 3,
        updated_at: now()?,
    };
    let mut recovery = MatterSubscriptionRecovery::from_stored(
        &stored,
        selection()?,
        0,
        5_000,
        true,
        recovery_policy()?,
    );
    let now = now()?;
    assert!(matches!(
        recovery.next_action(now),
        MatterSubscriptionRecoveryAction::MarkStale {
            reason: MatterSubscriptionLossReason::ControllerRestarted
        }
    ));
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::StalePersisted, now)?;
    let MatterSubscriptionRecoveryAction::GapRead(read) = recovery.next_action(now) else {
        return Err("first sleepy-device recovery read should be allowed".into());
    };
    assert_eq!(read.selection.paths().len(), 1);
    recovery.record_outcome(MatterSubscriptionRecoveryOutcome::GapReadCompleted, now)?;
    let MatterSubscriptionRecoveryAction::Resubscribe(request) = recovery.next_action(now) else {
        return Err("restart should restore logical subscription".into());
    };
    assert_eq!(request.subscription_id, subscription_id);
    assert_eq!(request.selection.paths().len(), 1);
    Ok(())
}

#[test]
fn data_version_wrap_should_be_treated_as_forward_progress() -> TestResult {
    let projection = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let projection = &projection.capabilities[0];
    let current = MatterReportedState::new(
        MatterStateValue::OnOff(false),
        Some(u32::MAX),
        1,
        now()?,
        now()?,
    )?;
    let wrapped = report(2, 0, true)?;

    assert!(matches!(
        normalize_matter_report(
            projection,
            &wrapped,
            now()? + TimeDelta::milliseconds(1),
            Some(&current),
            ObservationSourceKind::Notification,
            MatterReportCausation {
                common: None,
                desired_revision: None,
            },
        ),
        MatterReportDecision::Applied { .. }
    ));
    Ok(())
}

#[test]
fn report_should_confirm_only_the_exact_causally_bound_desired_revision() -> TestResult {
    let projection = project_matter_node(&installation_id()?, &light_descriptor(1)?);
    let projection = &projection.capabilities[0];
    let desired = MatterDesiredState::new(
        MatterStateRevision::new(7)?,
        MatterStateValue::OnOff(true),
        now()?,
    )?;
    let state = MatterProjectedState::new(
        MatterProjectionId::from_key(&fabric_id()?, 42, 1, "on_off", 1),
        Some(desired),
        None,
        None,
        MatterStateFreshness::Unknown,
        MatterConvergence::Pending,
        None,
    )?;
    let reported = applied_state(normalize_matter_report(
        projection,
        &report(20, 30, true)?,
        now()? + TimeDelta::milliseconds(1),
        None,
        ObservationSourceKind::Notification,
        MatterReportCausation {
            common: None,
            desired_revision: Some(7),
        },
    ))?;
    let confirmed = advance_matter_projected_state(
        &state,
        reported.clone(),
        &MatterReportCausation {
            common: None,
            desired_revision: Some(7),
        },
    )?;
    let unbound = advance_matter_projected_state(
        &state,
        reported,
        &MatterReportCausation {
            common: None,
            desired_revision: None,
        },
    )?;

    assert_eq!(confirmed.convergence(), MatterConvergence::Confirmed);
    assert_eq!(
        confirmed.confirmed_revision(),
        Some(MatterStateRevision::new(7)?)
    );
    assert_eq!(unbound.convergence(), MatterConvergence::Pending);
    assert_eq!(unbound.confirmed_revision(), None);
    Ok(())
}

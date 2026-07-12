//! Reusable controller-contract evidence for the deterministic simulator.

use std::error::Error;
use std::str::FromStr;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    MatterCancellationOutcome, MatterCommissioningRequest, MatterController,
    MatterControllerCommand, MatterCreateFabricRequest, MatterExportRequest,
    MatterFabricExportFormat, MatterFabricSecretRefs, MatterInvokeRequest, MatterReadRequest,
    MatterRemovalOutcome, MatterRemoveNodeRequest, MatterRestoreRequest, MatterSubscriptionRequest,
    SecretValue,
};
use homemagic_domain::{
    MatterAttributePath, MatterAttributeValue, MatterControllerError,
    MatterControllerErrorCategory, MatterControllerErrorCode, MatterControllerEventKind,
    MatterEndpointNumber, MatterFabricId, MatterLockState, MatterNodeId, MatterOperationId,
    MatterOperationPhase, MatterProjectionId, MatterRetryability, MatterStateRevision,
    MatterSubscriptionId, MatterSubscriptionLossReason, SecretRef,
};
use homemagic_matter::{
    DOOR_LOCK_CLUSTER_ID, DOOR_LOCK_STATE_ATTRIBUTE_ID, DeterministicMatterSimulator,
    ON_OFF_ATTRIBUTE_ID, ON_OFF_CLUSTER_ID, SIMULATOR_LIGHT_SETUP, SIMULATOR_LOCK_SETUP,
    SimulatorFault, SimulatorOperation, SimulatorReportFault,
};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use sha2::{Digest, Sha256};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct ReadySimulator {
    simulator: DeterministicMatterSimulator,
    fabric_id: MatterFabricId,
    light_node: MatterNodeId,
    lock_node: MatterNodeId,
    light_path: MatterAttributePath,
    lock_path: MatterAttributePath,
    light_operation_id: MatterOperationId,
}

fn started_at() -> TestResult<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")?.with_timezone(&Utc))
}

fn fabric_id() -> TestResult<MatterFabricId> {
    Ok(MatterFabricId::from_str(
        "10000000-0000-4000-8000-000000000001",
    )?)
}

fn operation_id(sequence: u8) -> TestResult<MatterOperationId> {
    Ok(MatterOperationId::from_str(&format!(
        "20000000-0000-4000-8000-{sequence:012}"
    ))?)
}

fn fabric_request(sequence: u8) -> TestResult<MatterCreateFabricRequest> {
    Ok(MatterCreateFabricRequest {
        operation_id: operation_id(sequence)?,
        fabric_id: fabric_id()?,
        secrets: MatterFabricSecretRefs {
            root_ca_key: SecretRef::from_backend_id("simulator-root-ref"),
            operational_key: SecretRef::from_backend_id("simulator-operational-ref"),
            controller_state: SecretRef::from_backend_id("simulator-state-ref"),
        },
    })
}

async fn ready() -> TestResult<ReadySimulator> {
    let simulator = DeterministicMatterSimulator::new(started_at()?);
    let request = fabric_request(1)?;
    simulator.create_fabric(request).await?;
    let light_operation_id = operation_id(2)?;
    let light = simulator
        .commission(MatterCommissioningRequest::new(
            light_operation_id.clone(),
            fabric_id()?,
            SecretValue::new(SIMULATOR_LIGHT_SETUP),
        ))
        .await?;
    let lock = simulator
        .commission(MatterCommissioningRequest::new(
            operation_id(3)?,
            fabric_id()?,
            SecretValue::new(SIMULATOR_LOCK_SETUP),
        ))
        .await?;
    let light_node = light.node_id();
    let lock_node = lock.node_id();
    Ok(ReadySimulator {
        simulator,
        fabric_id: fabric_id()?,
        light_node,
        lock_node,
        light_path: MatterAttributePath {
            node_id: light_node,
            endpoint: MatterEndpointNumber::new(1),
            cluster_id: ON_OFF_CLUSTER_ID,
            attribute_id: ON_OFF_ATTRIBUTE_ID,
        },
        lock_path: MatterAttributePath {
            node_id: lock_node,
            endpoint: MatterEndpointNumber::new(1),
            cluster_id: DOOR_LOCK_CLUSTER_ID,
            attribute_id: DOOR_LOCK_STATE_ATTRIBUTE_ID,
        },
        light_operation_id,
    })
}

fn selection(
    path: MatterAttributePath,
) -> TestResult<homemagic_application::MatterAttributeSelection> {
    Ok(homemagic_application::MatterAttributeSelection::new(vec![
        path,
    ])?)
}

fn invoke(
    setup: &ReadySimulator,
    node_id: MatterNodeId,
    rule: &str,
    revision: u64,
    command: MatterControllerCommand,
) -> TestResult<MatterInvokeRequest> {
    Ok(MatterInvokeRequest::new(
        MatterProjectionId::from_key(&setup.fabric_id, node_id.get(), 1, rule, 1),
        setup.fabric_id.clone(),
        node_id,
        MatterEndpointNumber::new(1),
        MatterStateRevision::new(revision)?,
        command,
    )?)
}

fn subscription_request(
    setup: &ReadySimulator,
    subscription_id: MatterSubscriptionId,
) -> TestResult<MatterSubscriptionRequest> {
    Ok(MatterSubscriptionRequest::new(
        subscription_id,
        setup.fabric_id.clone(),
        setup.light_node,
        selection(setup.light_path)?,
        0,
        1_000,
    )?)
}

async fn read_value(
    setup: &ReadySimulator,
    node_id: MatterNodeId,
    path: MatterAttributePath,
) -> TestResult<MatterAttributeValue> {
    let reports = setup
        .simulator
        .read(MatterReadRequest {
            fabric_id: setup.fabric_id.clone(),
            node_id,
            selection: selection(path)?,
        })
        .await?;
    reports
        .as_slice()
        .first()
        .map(|report| report.value.clone())
        .ok_or_else(|| "expected one report".into())
}

#[tokio::test]
async fn light_and_lock_should_exercise_every_happy_path_port_operation() -> TestResult {
    let setup = ready().await?;
    let status = setup
        .simulator
        .fabric_status(&setup.fabric_id)
        .await?
        .ok_or("fabric status missing")?;
    let nodes = setup.simulator.nodes(&setup.fabric_id).await?;
    let lock = setup
        .simulator
        .node(&setup.fabric_id, setup.lock_node)
        .await?;
    let subscription_id = MatterSubscriptionId::from_node(&setup.fabric_id, setup.light_node.get());
    setup
        .simulator
        .subscribe(MatterSubscriptionRequest::new(
            subscription_id,
            setup.fabric_id.clone(),
            setup.light_node,
            selection(setup.light_path)?,
            0,
            1_000,
        )?)
        .await?;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            1,
            MatterControllerCommand::SetOnOff(true),
        )?)
        .await?;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.lock_node,
            "door_lock",
            1,
            MatterControllerCommand::SetLock(MatterLockState::Unlocked),
        )?)
        .await?;
    let light_value = read_value(&setup, setup.light_node, setup.light_path).await?;
    let lock_value = read_value(&setup, setup.lock_node, setup.lock_path).await?;
    let events = setup.simulator.events_after(0, 256).await?;
    let export = setup
        .simulator
        .export_fabric(MatterExportRequest {
            operation_id: operation_id(4)?,
            fabric_id: setup.fabric_id.clone(),
        })
        .await?;
    let restored = DeterministicMatterSimulator::new(started_at()?);
    let restored_status = restored
        .restore_fabric(MatterRestoreRequest::new(
            operation_id(5)?,
            setup.fabric_id.clone(),
            export.format,
            SecretValue::new(export.envelope().to_vec()),
            SecretValue::new(export.recovery_key().to_vec()),
        ))
        .await?;
    let cancellation = setup
        .simulator
        .cancel_commissioning(&setup.light_operation_id)
        .await?;
    let removal = setup
        .simulator
        .remove_node(MatterRemoveNodeRequest {
            operation_id: operation_id(6)?,
            fabric_id: setup.fabric_id.clone(),
            node_id: setup.light_node,
        })
        .await?;

    assert_eq!(status.node_count, 2);
    assert_eq!(nodes.as_slice().len(), 2);
    assert!(lock.is_some());
    assert_eq!(light_value, MatterAttributeValue::Boolean(true));
    assert_eq!(lock_value, MatterAttributeValue::Unsigned(2));
    assert!(!events.events().is_empty());
    assert_eq!(export.format, MatterFabricExportFormat::SimulatorV1);
    assert_eq!(restored_status.node_count, 2);
    assert_eq!(cancellation, MatterCancellationOutcome::AlreadyCompleted);
    assert_eq!(removal, MatterRemovalOutcome::Removed);
    Ok(())
}

#[tokio::test]
async fn dispatch_barriers_should_separate_pre_dispatch_from_post_acknowledgement() -> TestResult {
    let setup = ready().await?;
    let barriers = setup.simulator.barriers();
    barriers.before_invoke.pause();
    let simulator = setup.simulator.clone();
    let request = invoke(
        &setup,
        setup.light_node,
        "on_off",
        1,
        MatterControllerCommand::SetOnOff(true),
    )?;
    let pending = tokio::spawn(async move { simulator.invoke(request).await });
    barriers.before_invoke.wait_until_reached().await;
    assert_eq!(
        read_value(&setup, setup.light_node, setup.light_path).await?,
        MatterAttributeValue::Boolean(false)
    );
    barriers.before_invoke.release();
    pending.await??;

    barriers.after_acknowledgement.pause();
    let simulator = setup.simulator.clone();
    let request = invoke(
        &setup,
        setup.light_node,
        "on_off",
        2,
        MatterControllerCommand::SetOnOff(false),
    )?;
    let acknowledged = tokio::spawn(async move { simulator.invoke(request).await });
    barriers.after_acknowledgement.wait_until_reached().await;
    assert_eq!(
        read_value(&setup, setup.light_node, setup.light_path).await?,
        MatterAttributeValue::Boolean(false)
    );
    let before_release = attribute_sequences(&setup.simulator).await?;
    barriers.after_acknowledgement.release();
    acknowledged.await??;
    let after_release = attribute_sequences(&setup.simulator).await?;

    assert_eq!(before_release.len() + 1, after_release.len());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the ordered scenario keeps report and subscription recovery facts visible"
)]
async fn report_and_subscription_faults_should_have_stable_recovery_behavior() -> TestResult {
    let setup = ready().await?;
    let subscription_id = MatterSubscriptionId::from_node(&setup.fabric_id, setup.light_node.get());
    setup
        .simulator
        .subscribe(subscription_request(&setup, subscription_id.clone())?)
        .await?;
    setup
        .simulator
        .inject_fault(SimulatorFault::Report(SimulatorReportFault::Drop))
        .await;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            1,
            MatterControllerCommand::SetOnOff(true),
        )?)
        .await?;
    assert!(attribute_sequences(&setup.simulator).await?.is_empty());

    setup
        .simulator
        .inject_fault(SimulatorFault::Report(SimulatorReportFault::Duplicate))
        .await;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            2,
            MatterControllerCommand::SetOnOff(false),
        )?)
        .await?;
    let duplicated = attribute_sequences(&setup.simulator).await?;
    assert_eq!(duplicated.len(), 2);
    assert_eq!(duplicated[0], duplicated[1]);

    let delayed_cursor = setup
        .simulator
        .events_after(0, 256)
        .await?
        .latest_cursor()
        .unwrap_or(0);
    setup
        .simulator
        .inject_fault(SimulatorFault::Report(SimulatorReportFault::Delay(
            TimeDelta::milliseconds(5),
        )))
        .await;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            3,
            MatterControllerCommand::SetOnOff(true),
        )?)
        .await?;
    assert!(
        attribute_sequences_after(&setup.simulator, delayed_cursor)
            .await?
            .is_empty()
    );
    setup.simulator.advance(TimeDelta::milliseconds(5)).await?;
    assert_eq!(
        attribute_sequences_after(&setup.simulator, delayed_cursor)
            .await?
            .len(),
        1
    );

    let cursor = setup
        .simulator
        .events_after(0, 256)
        .await?
        .latest_cursor()
        .unwrap_or(0);
    setup
        .simulator
        .inject_fault(SimulatorFault::Report(SimulatorReportFault::OutOfOrder))
        .await;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            4,
            MatterControllerCommand::SetOnOff(true),
        )?)
        .await?;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            5,
            MatterControllerCommand::SetOnOff(false),
        )?)
        .await?;
    setup.simulator.advance(TimeDelta::milliseconds(1)).await?;
    let reordered = attribute_sequences_after(&setup.simulator, cursor).await?;
    assert!(reordered[0] > reordered[1]);

    setup
        .simulator
        .inject_fault(SimulatorFault::SubscriptionLoss(
            MatterSubscriptionLossReason::SessionClosed,
        ))
        .await;
    setup
        .simulator
        .invoke(invoke(
            &setup,
            setup.light_node,
            "on_off",
            6,
            MatterControllerCommand::SetOnOff(true),
        )?)
        .await?;
    let page = setup.simulator.events_after(0, 256).await?;
    assert!(page.events().iter().any(|event| matches!(
        event.event.kind,
        MatterControllerEventKind::SubscriptionLost { .. }
    )));
    let restored = setup
        .simulator
        .subscribe(subscription_request(&setup, subscription_id)?)
        .await?;
    assert!(restored.established);
    Ok(())
}

#[tokio::test]
async fn explicit_outcome_and_read_faults_should_not_hide_state() -> TestResult {
    let setup = ready().await?;
    setup
        .simulator
        .inject_fault(SimulatorFault::UnknownCancellation)
        .await;
    let cancellation = setup
        .simulator
        .cancel_commissioning(&operation_id(9)?)
        .await?;
    setup
        .simulator
        .inject_fault(SimulatorFault::PartialRemoval)
        .await;
    let removal = setup
        .simulator
        .remove_node(MatterRemoveNodeRequest {
            operation_id: operation_id(10)?,
            fabric_id: setup.fabric_id.clone(),
            node_id: setup.light_node,
        })
        .await?;
    let expected = MatterControllerError::new(
        MatterControllerErrorCategory::Protocol,
        MatterControllerErrorCode::ReadFailed,
        MatterRetryability::Safe,
        None,
        None,
    );
    setup
        .simulator
        .inject_fault(SimulatorFault::FailNext {
            operation: SimulatorOperation::Read,
            error: expected.clone(),
        })
        .await;
    let read = setup
        .simulator
        .read(MatterReadRequest {
            fabric_id: setup.fabric_id.clone(),
            node_id: setup.light_node,
            selection: selection(setup.light_path)?,
        })
        .await;
    let node = setup
        .simulator
        .node(&setup.fabric_id, setup.light_node)
        .await?;

    assert_eq!(cancellation, MatterCancellationOutcome::OutcomeUnknown);
    assert_eq!(removal, MatterRemovalOutcome::PartialOutcome);
    assert_eq!(read, Err(expected));
    assert!(node.is_some());
    Ok(())
}

#[tokio::test]
async fn every_lifecycle_restart_phase_should_capture_resumable_state() -> TestResult {
    let phases = [
        MatterOperationPhase::ValidatingSetup,
        MatterOperationPhase::Discovering,
        MatterOperationPhase::EstablishingSession,
        MatterOperationPhase::Commissioning,
        MatterOperationPhase::Projecting,
        MatterOperationPhase::Subscribing,
        MatterOperationPhase::Cancelling,
        MatterOperationPhase::RemovingNode,
        MatterOperationPhase::CleaningSecrets,
        MatterOperationPhase::Exporting,
        MatterOperationPhase::Restoring,
        MatterOperationPhase::LoadingFabric,
    ];
    for phase in phases {
        let (simulator, operation_id, result) = restart_at(phase).await?;
        let Err(error) = result else {
            return Err("restart fault did not interrupt the operation".into());
        };
        let checkpoint = simulator
            .take_restart_checkpoint()
            .await
            .ok_or("restart checkpoint missing")?;
        let resumed = DeterministicMatterSimulator::from_restart_checkpoint(&checkpoint)?;

        assert_eq!(error.code, MatterControllerErrorCode::OutcomeIndeterminate);
        assert_eq!(checkpoint.operation_id, operation_id);
        assert_eq!(checkpoint.phase, phase);
        assert_eq!(resumed.implementation(), simulator.implementation());
    }
    Ok(())
}

#[tokio::test]
async fn protected_format_should_be_rejected_by_simulator_import() -> TestResult {
    let simulator = DeterministicMatterSimulator::new(started_at()?);
    let result = simulator
        .restore_fabric(MatterRestoreRequest::new(
            operation_id(9)?,
            fabric_id()?,
            MatterFabricExportFormat::ProtectedV1,
            SecretValue::new("not-a-simulator-envelope"),
            SecretValue::new("not-a-simulator-key"),
        ))
        .await;

    assert!(matches!(
        result,
        Err(MatterControllerError {
            code: MatterControllerErrorCode::UnsupportedOperation,
            ..
        })
    ));
    Ok(())
}

async fn attribute_sequences(simulator: &DeterministicMatterSimulator) -> TestResult<Vec<u64>> {
    attribute_sequences_after(simulator, 0).await
}

async fn attribute_sequences_after(
    simulator: &DeterministicMatterSimulator,
    cursor: u64,
) -> TestResult<Vec<u64>> {
    Ok(simulator
        .events_after(cursor, 256)
        .await?
        .events()
        .iter()
        .filter_map(|event| match &event.event.kind {
            MatterControllerEventKind::AttributeReport { report, .. } => {
                Some(report.report_sequence)
            }
            _ => None,
        })
        .collect())
}

async fn restart_at(
    phase: MatterOperationPhase,
) -> TestResult<(
    DeterministicMatterSimulator,
    MatterOperationId,
    Result<(), MatterControllerError>,
)> {
    let operation = operation_id(8)?;
    match phase {
        MatterOperationPhase::ValidatingSetup
        | MatterOperationPhase::Discovering
        | MatterOperationPhase::EstablishingSession
        | MatterOperationPhase::Commissioning
        | MatterOperationPhase::Projecting
        | MatterOperationPhase::Subscribing => {
            let simulator = DeterministicMatterSimulator::new(started_at()?);
            simulator.create_fabric(fabric_request(1)?).await?;
            simulator
                .inject_fault(SimulatorFault::RestartAt(phase))
                .await;
            let result = simulator
                .commission(MatterCommissioningRequest::new(
                    operation.clone(),
                    fabric_id()?,
                    SecretValue::new(SIMULATOR_LIGHT_SETUP),
                ))
                .await
                .map(|_| ());
            Ok((simulator, operation, result))
        }
        MatterOperationPhase::Cancelling => {
            let simulator = DeterministicMatterSimulator::new(started_at()?);
            simulator.create_fabric(fabric_request(1)?).await?;
            simulator
                .inject_fault(SimulatorFault::RestartAt(phase))
                .await;
            let result = simulator.cancel_commissioning(&operation).await.map(|_| ());
            Ok((simulator, operation, result))
        }
        MatterOperationPhase::RemovingNode | MatterOperationPhase::CleaningSecrets => {
            let setup = ready().await?;
            setup
                .simulator
                .inject_fault(SimulatorFault::RestartAt(phase))
                .await;
            let result = setup
                .simulator
                .remove_node(MatterRemoveNodeRequest {
                    operation_id: operation.clone(),
                    fabric_id: setup.fabric_id,
                    node_id: setup.light_node,
                })
                .await
                .map(|_| ());
            Ok((setup.simulator, operation, result))
        }
        MatterOperationPhase::Exporting => {
            let setup = ready().await?;
            setup
                .simulator
                .inject_fault(SimulatorFault::RestartAt(phase))
                .await;
            let result = setup
                .simulator
                .export_fabric(MatterExportRequest {
                    operation_id: operation.clone(),
                    fabric_id: setup.fabric_id,
                })
                .await
                .map(|_| ());
            Ok((setup.simulator, operation, result))
        }
        MatterOperationPhase::Restoring | MatterOperationPhase::LoadingFabric => {
            let source = ready().await?;
            let export = source
                .simulator
                .export_fabric(MatterExportRequest {
                    operation_id: operation_id(7)?,
                    fabric_id: source.fabric_id.clone(),
                })
                .await?;
            let simulator = DeterministicMatterSimulator::new(started_at()?);
            simulator
                .inject_fault(SimulatorFault::RestartAt(phase))
                .await;
            let result = simulator
                .restore_fabric(MatterRestoreRequest::new(
                    operation.clone(),
                    source.fabric_id,
                    export.format,
                    SecretValue::new(export.envelope().to_vec()),
                    SecretValue::new(export.recovery_key().to_vec()),
                ))
                .await
                .map(|_| ());
            Ok((simulator, operation, result))
        }
        _ => Err("phase is not a scripted lifecycle restart target".into()),
    }
}

async fn run_sequence(values: &[bool]) -> TestResult<Vec<u8>> {
    let setup = ready().await?;
    for (index, value) in values.iter().enumerate() {
        setup
            .simulator
            .invoke(invoke(
                &setup,
                setup.light_node,
                "on_off",
                u64::try_from(index)?.saturating_add(1),
                MatterControllerCommand::SetOnOff(*value),
            )?)
            .await?;
    }
    setup
        .simulator
        .normalized_trace_json()
        .await
        .map_err(Into::into)
}

#[tokio::test]
async fn normalized_fixture_trace_should_match_committed_hash() -> TestResult {
    let trace = run_sequence(&[true, false, true]).await?;
    let digest = hex_digest(Sha256::digest(&trace).as_slice());
    let expected = include_str!("fixtures/light-trace-v1.sha256").trim();
    let expected_trace = include_str!("fixtures/light-trace-v1.json").trim();

    assert_eq!(digest, expected);
    assert_eq!(trace, expected_trace.as_bytes());
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

proptest! {
    #[test]
    fn randomized_command_order_should_remain_byte_deterministic(
        values in prop::collection::vec(any::<bool>(), 0..16)
    ) {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let first = runtime
            .block_on(run_sequence(&values))
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let second = runtime
            .block_on(run_sequence(&values))
            .map_err(|error| TestCaseError::fail(error.to_string()))?;

        prop_assert_eq!(first, second);
    }
}

#[test]
fn every_error_category_should_be_injectable_without_text_payloads() -> TestResult {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        for category in [
            MatterControllerErrorCategory::Validation,
            MatterControllerErrorCategory::Discovery,
            MatterControllerErrorCategory::Transport,
            MatterControllerErrorCategory::Attestation,
            MatterControllerErrorCategory::Authentication,
            MatterControllerErrorCategory::Conflict,
            MatterControllerErrorCategory::NotFound,
            MatterControllerErrorCategory::Unsupported,
            MatterControllerErrorCategory::Timeout,
            MatterControllerErrorCategory::Cancelled,
            MatterControllerErrorCategory::SecretStore,
            MatterControllerErrorCategory::Persistence,
            MatterControllerErrorCategory::Protocol,
            MatterControllerErrorCategory::Internal,
        ] {
            let simulator = DeterministicMatterSimulator::new(started_at()?);
            let error = MatterControllerError::new(
                category,
                MatterControllerErrorCode::InternalInvariant,
                MatterRetryability::Never,
                None,
                None,
            );
            simulator
                .inject_fault(SimulatorFault::FailNext {
                    operation: SimulatorOperation::FabricStatus,
                    error: error.clone(),
                })
                .await;
            let found = simulator.fabric_status(&fabric_id()?).await;
            assert_eq!(found, Err(error));
        }
        TestResult::Ok(())
    })
}

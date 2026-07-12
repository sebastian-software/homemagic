use std::collections::{BTreeSet, VecDeque};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use homemagic_application::{
    MatterCancellationOutcome, MatterCommissioningRequest, MatterController,
    MatterControllerCommand, MatterControllerItems, MatterCreateFabricRequest, MatterCursorEvent,
    MatterEventPage, MatterExportRequest, MatterFabricExport, MatterFabricExportFormat,
    MatterFabricSecretRefs, MatterFabricState, MatterFabricStatus, MatterInvocationAcknowledgement,
    MatterInvokeRequest, MatterReadRequest, MatterRemovalOutcome, MatterRemoveNodeRequest,
    MatterRestoreRequest, MatterSubscriptionRequest, MatterSubscriptionStatus, SecretValue,
};
use homemagic_domain::{
    MatterAffectedResource, MatterAttributePath, MatterAttributeReport, MatterAttributeValue,
    MatterControllerError, MatterControllerErrorCategory, MatterControllerErrorCode,
    MatterControllerEvent, MatterControllerEventId, MatterControllerEventKind, MatterFabricId,
    MatterLockState, MatterNodeDescriptor, MatterNodeId, MatterOperationId, MatterOperationPhase,
    MatterRepairAction, MatterRetryability, MatterSubscriptionId, MatterSubscriptionLossReason,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::barrier::SimulatorDispatchBarriers;
use crate::clock::{SimulatorClock, SimulatorClockError};
use crate::fixture::{
    DOOR_LOCK_CLUSTER_ID, DOOR_LOCK_STATE_ATTRIBUTE_ID, ON_OFF_ATTRIBUTE_ID, ON_OFF_CLUSTER_ID,
    SimulatorFixture,
};
use crate::script::{
    SimulatorFault, SimulatorOperation, SimulatorReportFault, SimulatorRestartCheckpoint,
    SimulatorTraceEntry, SimulatorTraceKind,
};
use crate::{SIMULATOR_LIGHT_SETUP, SIMULATOR_LOCK_SETUP};

const CHECKPOINT_FORMAT: &str = "homemagic-matter-simulator-checkpoint-v1";
const EXPORT_PREFIX: &[u8] = b"HOMEMAGIC_SIMULATOR_EXPORT_V1\0";
const RECOVERY_KEY: &[u8] = b"SIMULATOR-NON-SECRET-RECOVERY-KEY-V1";
const MAX_EVENT_PAGE: usize = 256;

/// Deterministic in-process implementation of the application Matter port.
#[derive(Clone)]
pub struct DeterministicMatterSimulator {
    state: std::sync::Arc<Mutex<SimulatorState>>,
    clock: SimulatorClock,
    barriers: SimulatorDispatchBarriers,
}

impl DeterministicMatterSimulator {
    /// Creates an empty simulator at explicit virtual time.
    #[must_use]
    pub fn new(started_at: DateTime<Utc>) -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(SimulatorState::default())),
            clock: SimulatorClock::new(started_at),
            barriers: SimulatorDispatchBarriers::default(),
        }
    }

    /// Restores a complete simulator checkpoint.
    ///
    /// # Errors
    ///
    /// Rejects another checkpoint format or malformed state.
    pub fn from_checkpoint(
        checkpoint: &SimulatorCheckpoint,
    ) -> Result<Self, SimulatorControlError> {
        if checkpoint.format != CHECKPOINT_FORMAT {
            return Err(SimulatorControlError::UnsupportedCheckpoint);
        }
        let persisted: PersistedSimulatorState = serde_json::from_slice(&checkpoint.state)?;
        Ok(Self {
            state: std::sync::Arc::new(Mutex::new(persisted.state)),
            clock: SimulatorClock::new(persisted.now),
            barriers: SimulatorDispatchBarriers::default(),
        })
    }

    /// Restores state captured by an injected lifecycle restart.
    ///
    /// # Errors
    ///
    /// Rejects malformed simulator-only restart state.
    pub fn from_restart_checkpoint(
        checkpoint: &SimulatorRestartCheckpoint,
    ) -> Result<Self, SimulatorControlError> {
        let persisted: PersistedSimulatorState = serde_json::from_slice(&checkpoint.state)?;
        Ok(Self {
            state: std::sync::Arc::new(Mutex::new(persisted.state)),
            clock: SimulatorClock::new(persisted.now),
            barriers: SimulatorDispatchBarriers::default(),
        })
    }

    /// Returns the shared virtual clock.
    #[must_use]
    pub fn clock(&self) -> SimulatorClock {
        self.clock.clone()
    }

    /// Returns controllable dispatch barriers.
    #[must_use]
    pub fn barriers(&self) -> SimulatorDispatchBarriers {
        self.barriers.clone()
    }

    /// Appends one ordered fault to the script.
    pub async fn inject_fault(&self, fault: SimulatorFault) {
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        state.trace(
            now,
            SimulatorTraceKind::FaultInjected {
                name: fault_name(&fault),
            },
        );
        state.faults.push_back(fault);
    }

    /// Advances virtual time and delivers due reports without sleeping.
    ///
    /// # Errors
    ///
    /// Rejects invalid virtual-clock transitions.
    pub async fn advance(&self, by: TimeDelta) -> Result<(), SimulatorControlError> {
        let now = self.clock.advance(by).await?;
        self.state.lock().await.flush_due_reports(now);
        Ok(())
    }

    /// Delivers reports already due at current virtual time.
    pub async fn flush_reports(&self) {
        let now = self.clock.now().await;
        self.state.lock().await.flush_due_reports(now);
    }

    /// Captures complete simulator-only state.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if internal state violates its contract.
    pub async fn checkpoint(&self) -> Result<SimulatorCheckpoint, SimulatorControlError> {
        let now = self.clock.now().await;
        let state = self.state.lock().await.clone_without_runtime_faults();
        Ok(SimulatorCheckpoint {
            format: CHECKPOINT_FORMAT,
            state: serde_json::to_vec(&PersistedSimulatorState { now, state })?,
        })
    }

    /// Takes the most recent scripted restart checkpoint.
    pub async fn take_restart_checkpoint(&self) -> Option<SimulatorRestartCheckpoint> {
        self.state.lock().await.restart_checkpoint.take()
    }

    /// Returns byte-stable JSON for all normalized trace facts.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if a trace violates its contract.
    pub async fn normalized_trace_json(&self) -> Result<Vec<u8>, SimulatorControlError> {
        Ok(serde_json::to_vec(&self.state.lock().await.trace)?)
    }

    async fn fail_if_scripted(
        &self,
        operation: SimulatorOperation,
    ) -> Result<(), MatterControllerError> {
        let mut state = self.state.lock().await;
        if let Some(index) = state.faults.iter().position(
            |fault| matches!(fault, SimulatorFault::FailNext { operation: item, .. } if *item == operation),
        ) && let Some(SimulatorFault::FailNext { error, .. }) = state.faults.remove(index)
        {
            return Err(error);
        }
        Ok(())
    }

    async fn operation_phase(
        &self,
        fabric_id: &MatterFabricId,
        operation_id: &MatterOperationId,
        phase: MatterOperationPhase,
    ) -> Result<(), MatterControllerError> {
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        state.trace(
            now,
            SimulatorTraceKind::OperationPhase {
                operation_id: operation_id.clone(),
                phase,
            },
        );
        state.emit_event(
            fabric_id,
            now,
            MatterControllerEventKind::OperationProgress {
                operation_id: operation_id.clone(),
                phase,
            },
        );
        if let Some(index) = state
            .faults
            .iter()
            .position(|fault| matches!(fault, SimulatorFault::RestartAt(item) if *item == phase))
        {
            let _removed = state.faults.remove(index);
            let checkpoint_state = state.clone_without_runtime_faults();
            let payload = serde_json::to_vec(&PersistedSimulatorState {
                now,
                state: checkpoint_state,
            })
            .map_err(|_| internal_error())?;
            state.restart_checkpoint = Some(SimulatorRestartCheckpoint {
                operation_id: operation_id.clone(),
                phase,
                occurred_at: now,
                state: payload,
            });
            state.trace(
                now,
                SimulatorTraceKind::Restarted {
                    operation_id: operation_id.clone(),
                    phase,
                },
            );
            return Err(MatterControllerError::new(
                MatterControllerErrorCategory::Persistence,
                MatterControllerErrorCode::OutcomeIndeterminate,
                MatterRetryability::AfterRepair,
                Some(MatterAffectedResource::Operation {
                    operation_id: operation_id.clone(),
                }),
                Some(MatterRepairAction::ReviewPartialCleanup),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl MatterController for DeterministicMatterSimulator {
    fn implementation(&self) -> &'static str {
        "homemagic-deterministic-simulator"
    }

    async fn fabric_status(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<Option<MatterFabricStatus>, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::FabricStatus)
            .await?;
        let now = self.clock.now().await;
        let state = self.state.lock().await;
        Ok(state.fabric(fabric_id).map(|fabric| MatterFabricStatus {
            fabric_id: fabric.id.clone(),
            state: MatterFabricState::Active,
            node_count: fabric.nodes.len(),
            verified_at: now,
        }))
    }

    async fn create_fabric(
        &self,
        request: MatterCreateFabricRequest,
    ) -> Result<MatterFabricStatus, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::CreateFabric)
            .await?;
        self.operation_phase(
            &request.fabric_id,
            &request.operation_id,
            MatterOperationPhase::CreatingFabric,
        )
        .await?;
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        if state.fabric(&request.fabric_id).is_some() {
            return Err(fabric_error(
                &request.fabric_id,
                MatterControllerErrorCategory::Conflict,
                MatterControllerErrorCode::FabricConflict,
            ));
        }
        state.fabrics.push(SimulatorFabric {
            id: request.fabric_id.clone(),
            secrets: request.secrets,
            nodes: Vec::new(),
        });
        state.trace(
            now,
            SimulatorTraceKind::FabricCreated {
                fabric_id: request.fabric_id.clone(),
            },
        );
        Ok(MatterFabricStatus {
            fabric_id: request.fabric_id,
            state: MatterFabricState::Active,
            node_count: 0,
            verified_at: now,
        })
    }

    async fn commission(
        &self,
        request: MatterCommissioningRequest,
    ) -> Result<MatterNodeDescriptor, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::Commission)
            .await?;
        let fixture = fixture_from_setup(request.setup_payload()).ok_or_else(|| {
            MatterControllerError::new(
                MatterControllerErrorCategory::Validation,
                MatterControllerErrorCode::InvalidSetupPayload,
                MatterRetryability::Never,
                Some(MatterAffectedResource::Fabric {
                    fabric_id: request.fabric_id().clone(),
                }),
                None,
            )
        })?;
        for phase in [
            MatterOperationPhase::ValidatingSetup,
            MatterOperationPhase::Discovering,
            MatterOperationPhase::EstablishingSession,
            MatterOperationPhase::Commissioning,
            MatterOperationPhase::Projecting,
            MatterOperationPhase::Subscribing,
        ] {
            self.operation_phase(request.fabric_id(), request.operation_id(), phase)
                .await?;
        }
        let materialized = fixture
            .materialize(request.fabric_id().clone())
            .map_err(|_| internal_error())?;
        let descriptor = materialized.descriptor.clone();
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        let fabric = state
            .fabric_mut(request.fabric_id())
            .ok_or_else(|| fabric_not_found(request.fabric_id()))?;
        if fabric
            .nodes
            .iter()
            .any(|node| node.descriptor.node_id() == descriptor.node_id())
        {
            return Err(MatterControllerError::new(
                MatterControllerErrorCategory::Conflict,
                MatterControllerErrorCode::FabricConflict,
                MatterRetryability::Never,
                Some(MatterAffectedResource::Node {
                    fabric_id: request.fabric_id().clone(),
                    node_id: descriptor.node_id(),
                }),
                None,
            ));
        }
        fabric.nodes.push(SimulatorNode {
            fixture: materialized.fixture,
            descriptor: descriptor.clone(),
            attributes: materialized
                .attributes
                .into_iter()
                .map(|(path, value)| SimulatorAttribute {
                    path,
                    value,
                    data_version: 1,
                })
                .collect(),
        });
        state
            .completed_commissioning
            .insert(request.operation_id().clone());
        state
            .operation_fabrics
            .push((request.operation_id().clone(), request.fabric_id().clone()));
        state.trace(
            now,
            SimulatorTraceKind::NodeCommissioned {
                fabric_id: request.fabric_id().clone(),
                node_id: descriptor.node_id().get(),
                fixture: fixture.key().to_owned(),
            },
        );
        Ok(descriptor)
    }

    async fn cancel_commissioning(
        &self,
        operation_id: &MatterOperationId,
    ) -> Result<MatterCancellationOutcome, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::CancelCommissioning)
            .await?;
        let fabric_id = {
            let state = self.state.lock().await;
            state
                .operation_fabrics
                .iter()
                .find(|(id, _)| id == operation_id)
                .map(|(_, fabric)| fabric.clone())
                .or_else(|| state.fabrics.first().map(|fabric| fabric.id.clone()))
                .ok_or_else(internal_error)?
        };
        self.operation_phase(&fabric_id, operation_id, MatterOperationPhase::Cancelling)
            .await?;
        let mut state = self.state.lock().await;
        if take_simple_fault(&mut state.faults, |fault| {
            matches!(fault, SimulatorFault::UnknownCancellation)
        }) {
            return Ok(MatterCancellationOutcome::OutcomeUnknown);
        }
        if state.completed_commissioning.contains(operation_id) {
            Ok(MatterCancellationOutcome::AlreadyCompleted)
        } else {
            Ok(MatterCancellationOutcome::Cancelled)
        }
    }

    async fn nodes(
        &self,
        fabric_id: &MatterFabricId,
    ) -> Result<MatterControllerItems<MatterNodeDescriptor>, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::Nodes).await?;
        let state = self.state.lock().await;
        let fabric = state
            .fabric(fabric_id)
            .ok_or_else(|| fabric_not_found(fabric_id))?;
        MatterControllerItems::new(
            fabric
                .nodes
                .iter()
                .map(|node| node.descriptor.clone())
                .collect(),
        )
        .map_err(|_| internal_error())
    }

    async fn node(
        &self,
        fabric_id: &MatterFabricId,
        node_id: MatterNodeId,
    ) -> Result<Option<MatterNodeDescriptor>, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::Node).await?;
        let state = self.state.lock().await;
        let fabric = state
            .fabric(fabric_id)
            .ok_or_else(|| fabric_not_found(fabric_id))?;
        Ok(fabric
            .nodes
            .iter()
            .find(|node| node.descriptor.node_id() == node_id)
            .map(|node| node.descriptor.clone()))
    }

    async fn subscribe(
        &self,
        request: MatterSubscriptionRequest,
    ) -> Result<MatterSubscriptionStatus, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::Subscribe).await?;
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        let fabric = state
            .fabric(&request.fabric_id)
            .ok_or_else(|| fabric_not_found(&request.fabric_id))?;
        let node = fabric
            .nodes
            .iter()
            .find(|node| node.descriptor.node_id() == request.node_id)
            .ok_or_else(|| node_not_found(&request.fabric_id, request.node_id))?;
        if !request.selection.paths().iter().all(|path| {
            node.attributes
                .iter()
                .any(|attribute| attribute.path == *path)
        }) {
            return Err(MatterControllerError::new(
                MatterControllerErrorCategory::Validation,
                MatterControllerErrorCode::InvalidRequest,
                MatterRetryability::Never,
                Some(MatterAffectedResource::Node {
                    fabric_id: request.fabric_id,
                    node_id: request.node_id,
                }),
                None,
            ));
        }
        let report_sequence = state.next_report_sequence;
        state
            .subscriptions
            .retain(|item| item.id != request.subscription_id);
        state.subscriptions.push(SimulatorSubscription {
            id: request.subscription_id.clone(),
            fabric_id: request.fabric_id,
            node_id: request.node_id,
            paths: request.selection.paths().to_vec(),
        });
        state.trace(
            now,
            SimulatorTraceKind::SubscriptionEstablished {
                subscription_id: request.subscription_id.to_string(),
            },
        );
        Ok(MatterSubscriptionStatus {
            subscription_id: request.subscription_id,
            established: true,
            report_sequence,
            verified_at: now,
        })
    }

    async fn read(
        &self,
        request: MatterReadRequest,
    ) -> Result<MatterControllerItems<MatterAttributeReport>, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::Read).await?;
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        let reports = {
            let fabric = state
                .fabric(&request.fabric_id)
                .ok_or_else(|| fabric_not_found(&request.fabric_id))?;
            let node = fabric
                .nodes
                .iter()
                .find(|node| node.descriptor.node_id() == request.node_id)
                .ok_or_else(|| node_not_found(&request.fabric_id, request.node_id))?;
            request
                .selection
                .paths()
                .iter()
                .map(|path| {
                    node.attributes
                        .iter()
                        .find(|attribute| attribute.path == *path)
                        .map(|attribute| MatterAttributeReport {
                            path: *path,
                            value: attribute.value.clone(),
                            data_version: Some(attribute.data_version),
                            report_sequence: state.next_report_sequence,
                            observed_at: now,
                        })
                        .ok_or_else(|| node_not_found(&request.fabric_id, request.node_id))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        state.trace(
            now,
            SimulatorTraceKind::ReadCompleted {
                report_count: reports.len(),
            },
        );
        MatterControllerItems::new(reports).map_err(|_| internal_error())
    }

    async fn invoke(
        &self,
        request: MatterInvokeRequest,
    ) -> Result<MatterInvocationAcknowledgement, MatterControllerError> {
        self.barriers.before_invoke.cross().await;
        self.fail_if_scripted(SimulatorOperation::Invoke).await?;
        let now = self.clock.now().await;
        let (report, report_fault, subscription_loss) = {
            let mut state = self.state.lock().await;
            let report_sequence = state
                .next_report_sequence
                .checked_add(1)
                .ok_or_else(internal_error)?;
            state.next_report_sequence = report_sequence;
            let report = mutate_for_invoke(&mut state, &request, now, report_sequence)?;
            let report_fault = take_report_fault(&mut state.faults);
            let subscription_loss = take_subscription_loss(&mut state.faults);
            state.trace(
                now,
                SimulatorTraceKind::InvocationAcknowledged {
                    projection_id: request.projection_id.to_string(),
                    desired_revision: request.desired_revision.get(),
                },
            );
            (report, report_fault, subscription_loss)
        };
        let acknowledgement = MatterInvocationAcknowledgement {
            acknowledged_at: now,
        };
        self.barriers.after_acknowledgement.cross().await;
        let mut state = self.state.lock().await;
        if let Some(reason) = subscription_loss {
            state.lose_subscriptions(now, reason);
        } else {
            state.schedule_report(&request.fabric_id, &report, report_fault.as_ref(), now);
            state.flush_due_reports(now);
        }
        Ok(acknowledgement)
    }

    async fn remove_node(
        &self,
        request: MatterRemoveNodeRequest,
    ) -> Result<MatterRemovalOutcome, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::RemoveNode)
            .await?;
        self.operation_phase(
            &request.fabric_id,
            &request.operation_id,
            MatterOperationPhase::RemovingNode,
        )
        .await?;
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        if take_simple_fault(&mut state.faults, |fault| {
            matches!(fault, SimulatorFault::PartialRemoval)
        }) {
            return Ok(MatterRemovalOutcome::PartialOutcome);
        }
        let Some(fabric) = state.fabric_mut(&request.fabric_id) else {
            return Ok(MatterRemovalOutcome::NotPresent);
        };
        let before = fabric.nodes.len();
        fabric
            .nodes
            .retain(|node| node.descriptor.node_id() != request.node_id);
        if fabric.nodes.len() == before {
            return Ok(MatterRemovalOutcome::NotPresent);
        }
        drop(state);
        self.operation_phase(
            &request.fabric_id,
            &request.operation_id,
            MatterOperationPhase::CleaningSecrets,
        )
        .await?;
        let mut state = self.state.lock().await;
        state.trace(
            now,
            SimulatorTraceKind::NodeRemoved {
                fabric_id: request.fabric_id,
                node_id: request.node_id.get(),
            },
        );
        Ok(MatterRemovalOutcome::Removed)
    }

    async fn export_fabric(
        &self,
        request: MatterExportRequest,
    ) -> Result<MatterFabricExport, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::ExportFabric)
            .await?;
        self.operation_phase(
            &request.fabric_id,
            &request.operation_id,
            MatterOperationPhase::Exporting,
        )
        .await?;
        let now = self.clock.now().await;
        let mut state = self.state.lock().await;
        let fabric = state
            .fabric(&request.fabric_id)
            .ok_or_else(|| fabric_not_found(&request.fabric_id))?
            .clone();
        let mut envelope = EXPORT_PREFIX.to_vec();
        envelope.extend(serde_json::to_vec(&fabric).map_err(|_| internal_error())?);
        state.trace(
            now,
            SimulatorTraceKind::FabricExported {
                fabric_id: request.fabric_id,
            },
        );
        Ok(MatterFabricExport::new(
            MatterFabricExportFormat::SimulatorV1,
            SecretValue::new(envelope),
            SecretValue::new(RECOVERY_KEY),
        ))
    }

    async fn restore_fabric(
        &self,
        request: MatterRestoreRequest,
    ) -> Result<MatterFabricStatus, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::RestoreFabric)
            .await?;
        if request.format() != MatterFabricExportFormat::SimulatorV1
            || request.recovery_key() != RECOVERY_KEY
            || !request.envelope().starts_with(EXPORT_PREFIX)
        {
            return Err(MatterControllerError::new(
                MatterControllerErrorCategory::Unsupported,
                MatterControllerErrorCode::UnsupportedOperation,
                MatterRetryability::Never,
                Some(MatterAffectedResource::Fabric {
                    fabric_id: request.expected_fabric_id().clone(),
                }),
                None,
            ));
        }
        self.operation_phase(
            request.expected_fabric_id(),
            request.operation_id(),
            MatterOperationPhase::Restoring,
        )
        .await?;
        let fabric: SimulatorFabric =
            serde_json::from_slice(&request.envelope()[EXPORT_PREFIX.len()..])
                .map_err(|_| internal_error())?;
        if &fabric.id != request.expected_fabric_id() {
            return Err(fabric_error(
                request.expected_fabric_id(),
                MatterControllerErrorCategory::Conflict,
                MatterControllerErrorCode::FabricConflict,
            ));
        }
        self.operation_phase(
            request.expected_fabric_id(),
            request.operation_id(),
            MatterOperationPhase::LoadingFabric,
        )
        .await?;
        let now = self.clock.now().await;
        let node_count = fabric.nodes.len();
        let mut state = self.state.lock().await;
        if state.fabric(&fabric.id).is_some() {
            return Err(fabric_error(
                &fabric.id,
                MatterControllerErrorCategory::Conflict,
                MatterControllerErrorCode::FabricConflict,
            ));
        }
        state.fabrics.push(fabric.clone());
        state.trace(
            now,
            SimulatorTraceKind::FabricRestored {
                fabric_id: fabric.id.clone(),
            },
        );
        Ok(MatterFabricStatus {
            fabric_id: fabric.id,
            state: MatterFabricState::Active,
            node_count,
            verified_at: now,
        })
    }

    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<MatterEventPage, MatterControllerError> {
        self.fail_if_scripted(SimulatorOperation::EventsAfter)
            .await?;
        let state = self.state.lock().await;
        let latest = u64::try_from(state.events.len()).map_err(|_| internal_error())?;
        let events = state
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let event_cursor = u64::try_from(index).ok()?.checked_add(1)?;
                (event_cursor > cursor).then(|| MatterCursorEvent {
                    cursor: event_cursor,
                    event: event.clone(),
                })
            })
            .take(limit.min(MAX_EVENT_PAGE))
            .collect();
        MatterEventPage::new(
            (!state.events.is_empty()).then_some(1),
            (!state.events.is_empty()).then_some(latest),
            events,
        )
        .map_err(|_| internal_error())
    }
}

/// Opaque complete simulator checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatorCheckpoint {
    format: &'static str,
    state: Vec<u8>,
}

impl SimulatorCheckpoint {
    /// Returns the simulator-only format identifier.
    #[must_use]
    pub const fn format(&self) -> &'static str {
        self.format
    }

    /// Returns checkpoint bytes for durable test-fixture storage.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.state
    }
}

/// Simulator control-plane failure outside the Matter port.
#[derive(Debug, Error)]
pub enum SimulatorControlError {
    /// Checkpoint belongs to another implementation or version.
    #[error("unsupported simulator checkpoint format")]
    UnsupportedCheckpoint,
    /// Checkpoint or trace serialization failed.
    #[error("invalid simulator state: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Virtual clock transition failed.
    #[error(transparent)]
    Clock(#[from] SimulatorClockError),
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct SimulatorState {
    fabrics: Vec<SimulatorFabric>,
    subscriptions: Vec<SimulatorSubscription>,
    events: Vec<MatterControllerEvent>,
    pending_reports: Vec<ScheduledReport>,
    next_report_sequence: u64,
    next_schedule_order: u64,
    next_trace_sequence: u64,
    trace: Vec<SimulatorTraceEntry>,
    completed_commissioning: BTreeSet<MatterOperationId>,
    operation_fabrics: Vec<(MatterOperationId, MatterFabricId)>,
    #[serde(skip)]
    faults: VecDeque<SimulatorFault>,
    #[serde(skip)]
    restart_checkpoint: Option<SimulatorRestartCheckpoint>,
}

impl SimulatorState {
    fn fabric(&self, fabric_id: &MatterFabricId) -> Option<&SimulatorFabric> {
        self.fabrics.iter().find(|fabric| &fabric.id == fabric_id)
    }

    fn fabric_mut(&mut self, fabric_id: &MatterFabricId) -> Option<&mut SimulatorFabric> {
        self.fabrics
            .iter_mut()
            .find(|fabric| &fabric.id == fabric_id)
    }

    fn trace(&mut self, occurred_at: DateTime<Utc>, kind: SimulatorTraceKind) {
        self.next_trace_sequence = self.next_trace_sequence.saturating_add(1);
        self.trace.push(SimulatorTraceEntry {
            sequence: self.next_trace_sequence,
            occurred_at,
            kind,
        });
    }

    fn emit_event(
        &mut self,
        fabric_id: &MatterFabricId,
        occurred_at: DateTime<Utc>,
        kind: MatterControllerEventKind,
    ) {
        let sequence = u64::try_from(self.events.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .unwrap_or(u64::MAX);
        self.events.push(MatterControllerEvent {
            id: MatterControllerEventId::from_sequence(fabric_id, sequence),
            occurred_at,
            kind,
        });
    }

    fn schedule_report(
        &mut self,
        fabric_id: &MatterFabricId,
        report: &MatterAttributeReport,
        fault: Option<&SimulatorReportFault>,
        now: DateTime<Utc>,
    ) {
        if fault == Some(&SimulatorReportFault::Drop) {
            self.trace(
                now,
                SimulatorTraceKind::ReportDropped {
                    report_sequence: report.report_sequence,
                },
            );
            return;
        }
        let delay = match fault {
            Some(SimulatorReportFault::Delay(value)) if *value > TimeDelta::zero() => *value,
            Some(SimulatorReportFault::OutOfOrder) => TimeDelta::milliseconds(1),
            _ => TimeDelta::zero(),
        };
        let deliver_at = now.checked_add_signed(delay).unwrap_or(now);
        let copies = if fault == Some(&SimulatorReportFault::Duplicate) {
            2
        } else {
            1
        };
        for _ in 0..copies {
            self.next_schedule_order = self.next_schedule_order.saturating_add(1);
            self.pending_reports.push(ScheduledReport {
                deliver_at,
                order: self.next_schedule_order,
                fabric_id: fabric_id.clone(),
                report: report.clone(),
            });
        }
        self.trace(
            now,
            SimulatorTraceKind::ReportScheduled {
                report_sequence: report.report_sequence,
                delay_millis: delay.num_milliseconds(),
            },
        );
    }

    fn flush_due_reports(&mut self, now: DateTime<Utc>) {
        self.pending_reports
            .sort_by_key(|report| (report.deliver_at, report.order));
        let mut due = Vec::new();
        let mut pending = Vec::new();
        for report in self.pending_reports.drain(..) {
            if report.deliver_at <= now {
                due.push(report);
            } else {
                pending.push(report);
            }
        }
        self.pending_reports = pending;
        for scheduled in due {
            self.emit_event(
                &scheduled.fabric_id,
                scheduled.deliver_at,
                MatterControllerEventKind::AttributeReport {
                    fabric_id: scheduled.fabric_id.clone(),
                    report: scheduled.report.clone(),
                },
            );
            self.trace(
                scheduled.deliver_at,
                SimulatorTraceKind::ReportDelivered {
                    report_sequence: scheduled.report.report_sequence,
                },
            );
        }
    }

    fn lose_subscriptions(&mut self, now: DateTime<Utc>, reason: MatterSubscriptionLossReason) {
        let subscriptions = std::mem::take(&mut self.subscriptions);
        for subscription in subscriptions {
            self.emit_event(
                &subscription.fabric_id,
                now,
                MatterControllerEventKind::SubscriptionLost {
                    subscription_id: subscription.id,
                    reason,
                },
            );
        }
        self.trace(now, SimulatorTraceKind::SubscriptionLost { reason });
    }

    fn clone_without_runtime_faults(&self) -> Self {
        let mut state = self.clone();
        state.faults.clear();
        state.restart_checkpoint = None;
        state
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct PersistedSimulatorState {
    now: DateTime<Utc>,
    state: SimulatorState,
}

#[derive(Clone, Serialize, Deserialize)]
struct SimulatorFabric {
    id: MatterFabricId,
    secrets: MatterFabricSecretRefs,
    nodes: Vec<SimulatorNode>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SimulatorNode {
    fixture: SimulatorFixture,
    descriptor: MatterNodeDescriptor,
    attributes: Vec<SimulatorAttribute>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SimulatorAttribute {
    path: MatterAttributePath,
    value: MatterAttributeValue,
    data_version: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct SimulatorSubscription {
    id: MatterSubscriptionId,
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    paths: Vec<MatterAttributePath>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ScheduledReport {
    deliver_at: DateTime<Utc>,
    order: u64,
    fabric_id: MatterFabricId,
    report: MatterAttributeReport,
}

fn fixture_from_setup(setup: &[u8]) -> Option<SimulatorFixture> {
    match setup {
        value if value == SIMULATOR_LIGHT_SETUP => Some(SimulatorFixture::LightV1),
        value if value == SIMULATOR_LOCK_SETUP => Some(SimulatorFixture::DoorLockV1),
        _ => None,
    }
}

fn mutate_for_invoke(
    state: &mut SimulatorState,
    request: &MatterInvokeRequest,
    now: DateTime<Utc>,
    report_sequence: u64,
) -> Result<MatterAttributeReport, MatterControllerError> {
    let fabric = state
        .fabric_mut(&request.fabric_id)
        .ok_or_else(|| fabric_not_found(&request.fabric_id))?;
    let node = fabric
        .nodes
        .iter_mut()
        .find(|node| node.descriptor.node_id() == request.node_id)
        .ok_or_else(|| node_not_found(&request.fabric_id, request.node_id))?;
    let (cluster_id, attribute_id, value) = match request.command {
        MatterControllerCommand::SetOnOff(value) => (
            ON_OFF_CLUSTER_ID,
            ON_OFF_ATTRIBUTE_ID,
            MatterAttributeValue::Boolean(value),
        ),
        MatterControllerCommand::SetLock(value) => (
            DOOR_LOCK_CLUSTER_ID,
            DOOR_LOCK_STATE_ATTRIBUTE_ID,
            MatterAttributeValue::Unsigned(match value {
                MatterLockState::Locked => 1,
                MatterLockState::Unlocked => 2,
                MatterLockState::NotFullyLocked | MatterLockState::Unknown => {
                    return Err(invalid_invoke(request));
                }
            }),
        ),
        MatterControllerCommand::SetLevelPercent(_)
        | MatterControllerCommand::SetPositionPercent(_)
        | MatterControllerCommand::Stop => return Err(invalid_invoke(request)),
    };
    let path = MatterAttributePath {
        node_id: request.node_id,
        endpoint: request.endpoint,
        cluster_id,
        attribute_id,
    };
    let attribute = node
        .attributes
        .iter_mut()
        .find(|attribute| attribute.path == path)
        .ok_or_else(|| invalid_invoke(request))?;
    attribute.value = value;
    attribute.data_version = attribute
        .data_version
        .checked_add(1)
        .ok_or_else(internal_error)?;
    Ok(MatterAttributeReport {
        path,
        value: attribute.value.clone(),
        data_version: Some(attribute.data_version),
        report_sequence,
        observed_at: now,
    })
}

fn take_report_fault(faults: &mut VecDeque<SimulatorFault>) -> Option<SimulatorReportFault> {
    let index = faults
        .iter()
        .position(|fault| matches!(fault, SimulatorFault::Report(_)))?;
    match faults.remove(index) {
        Some(SimulatorFault::Report(fault)) => Some(fault),
        _ => None,
    }
}

fn take_subscription_loss(
    faults: &mut VecDeque<SimulatorFault>,
) -> Option<MatterSubscriptionLossReason> {
    let index = faults
        .iter()
        .position(|fault| matches!(fault, SimulatorFault::SubscriptionLoss(_)))?;
    match faults.remove(index) {
        Some(SimulatorFault::SubscriptionLoss(reason)) => Some(reason),
        _ => None,
    }
}

fn take_simple_fault(
    faults: &mut VecDeque<SimulatorFault>,
    predicate: impl Fn(&SimulatorFault) -> bool,
) -> bool {
    if let Some(index) = faults.iter().position(predicate) {
        let _removed = faults.remove(index);
        true
    } else {
        false
    }
}

fn fault_name(fault: &SimulatorFault) -> String {
    match fault {
        SimulatorFault::FailNext { operation, .. } => format!("fail_next:{operation:?}"),
        SimulatorFault::Report(report) => format!("report:{report:?}"),
        SimulatorFault::SubscriptionLoss(reason) => format!("subscription_loss:{reason:?}"),
        SimulatorFault::PartialRemoval => "partial_removal".to_owned(),
        SimulatorFault::UnknownCancellation => "unknown_cancellation".to_owned(),
        SimulatorFault::RestartAt(phase) => format!("restart_at:{phase:?}"),
    }
}

fn fabric_not_found(fabric_id: &MatterFabricId) -> MatterControllerError {
    fabric_error(
        fabric_id,
        MatterControllerErrorCategory::NotFound,
        MatterControllerErrorCode::FabricNotFound,
    )
}

fn fabric_error(
    fabric_id: &MatterFabricId,
    category: MatterControllerErrorCategory,
    code: MatterControllerErrorCode,
) -> MatterControllerError {
    MatterControllerError::new(
        category,
        code,
        MatterRetryability::Never,
        Some(MatterAffectedResource::Fabric {
            fabric_id: fabric_id.clone(),
        }),
        None,
    )
}

fn node_not_found(fabric_id: &MatterFabricId, node_id: MatterNodeId) -> MatterControllerError {
    MatterControllerError::new(
        MatterControllerErrorCategory::NotFound,
        MatterControllerErrorCode::NodeNotFound,
        MatterRetryability::Never,
        Some(MatterAffectedResource::Node {
            fabric_id: fabric_id.clone(),
            node_id,
        }),
        None,
    )
}

fn invalid_invoke(request: &MatterInvokeRequest) -> MatterControllerError {
    MatterControllerError::new(
        MatterControllerErrorCategory::Validation,
        MatterControllerErrorCode::InvalidRequest,
        MatterRetryability::Never,
        Some(MatterAffectedResource::Endpoint {
            fabric_id: request.fabric_id.clone(),
            node_id: request.node_id,
            endpoint: request.endpoint,
        }),
        None,
    )
}

fn internal_error() -> MatterControllerError {
    MatterControllerError::new(
        MatterControllerErrorCategory::Internal,
        MatterControllerErrorCode::InternalInvariant,
        MatterRetryability::Never,
        Some(MatterAffectedResource::Controller),
        Some(MatterRepairAction::UpdateControllerAdapter),
    )
}

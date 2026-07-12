//! Daemon-owned execution queue for durable Matter operations.

use chrono::Utc;
use homemagic_domain::{Actor, MatterOperation, MatterOperationId};
use thiserror::Error;

use crate::{
    MatterCancellationResult, MatterCommissioningInput, MatterFabricWorkflowError,
    MatterFabricWorkflowService, MatterNodeWorkflowError, MatterNodeWorkflowService,
    MatterSimulatorExport, MatterSimulatorRestoreInput, MatterSubscriptionRepairError,
    MatterSubscriptionRepairService, MatterWorkflowOutcome,
};

/// Cloneable transport-to-daemon handoff for Matter operation execution.
#[derive(Clone)]
pub struct MatterExecutionHandle {
    sender: tokio::sync::mpsc::Sender<MatterExecutionRequest>,
}

/// Daemon-owned receiver and workflow composition.
pub struct MatterExecutionWorker {
    receiver: tokio::sync::mpsc::Receiver<MatterExecutionRequest>,
    fabric: MatterFabricWorkflowService,
    nodes: MatterNodeWorkflowService,
    subscriptions: MatterSubscriptionRepairService,
}

/// Explicit failure at the bounded execution handoff or workflow boundary.
#[derive(Debug, Error)]
pub enum MatterExecutionError {
    /// The daemon receiver is unavailable.
    #[error("Matter operation worker is unavailable")]
    Unavailable,
    /// The bounded daemon queue is full.
    #[error("Matter operation worker queue is full")]
    Busy,
    /// A fabric workflow failed outside its normalized durable terminal path.
    #[error("Matter fabric workflow failed")]
    Fabric(#[from] MatterFabricWorkflowError),
    /// A node workflow failed outside its normalized durable terminal path.
    #[error("Matter node workflow failed")]
    Node(#[from] MatterNodeWorkflowError),
    /// A subscription workflow failed outside its normalized durable terminal path.
    #[error("Matter subscription workflow failed")]
    Subscription(#[from] MatterSubscriptionRepairError),
}

/// Sensitive export result delivered only to the dedicated transport path.
pub struct MatterSensitiveExport {
    /// Terminal durable operation.
    pub operation: MatterOperation,
    /// Non-serializable simulator export bytes.
    pub export: Option<MatterSimulatorExport>,
}

enum MatterExecutionRequest {
    Create {
        actor: Actor,
        operation_id: MatterOperationId,
    },
    Remove {
        actor: Actor,
        operation_id: MatterOperationId,
    },
    Cancel {
        actor: Actor,
        operation_id: MatterOperationId,
    },
    Repair {
        actor: Actor,
        operation_id: MatterOperationId,
    },
    Commission {
        actor: Actor,
        operation_id: MatterOperationId,
        input: MatterCommissioningInput,
        response: tokio::sync::oneshot::Sender<Result<MatterOperation, MatterExecutionError>>,
    },
    Export {
        actor: Actor,
        operation_id: MatterOperationId,
        response: tokio::sync::oneshot::Sender<Result<MatterSensitiveExport, MatterExecutionError>>,
    },
    Restore {
        actor: Actor,
        operation_id: MatterOperationId,
        input: MatterSimulatorRestoreInput,
        response: tokio::sync::oneshot::Sender<Result<MatterOperation, MatterExecutionError>>,
    },
}

impl MatterExecutionHandle {
    /// Creates one bounded execution handoff and its daemon-owned receiver.
    #[must_use]
    pub fn channel(
        capacity: usize,
        fabric: MatterFabricWorkflowService,
        nodes: MatterNodeWorkflowService,
        subscriptions: MatterSubscriptionRepairService,
    ) -> (Self, MatterExecutionWorker) {
        let (sender, receiver) = tokio::sync::mpsc::channel(capacity.max(1));
        (
            Self { sender },
            MatterExecutionWorker {
                receiver,
                fabric,
                nodes,
                subscriptions,
            },
        )
    }

    /// Wakes daemon-owned execution of one ordinary durable operation.
    ///
    /// # Errors
    ///
    /// Returns busy or unavailable without changing the already durable operation.
    pub fn wake_create(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
    ) -> Result<(), MatterExecutionError> {
        self.try_send(MatterExecutionRequest::Create {
            actor,
            operation_id,
        })
    }

    /// Wakes daemon-owned node removal.
    ///
    /// # Errors
    ///
    /// Returns busy or unavailable without changing the already durable operation.
    pub fn wake_remove(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
    ) -> Result<(), MatterExecutionError> {
        self.try_send(MatterExecutionRequest::Remove {
            actor,
            operation_id,
        })
    }

    /// Wakes daemon-owned commissioning cancellation.
    ///
    /// # Errors
    ///
    /// Returns busy or unavailable without changing the already durable operation.
    pub fn wake_cancel(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
    ) -> Result<(), MatterExecutionError> {
        self.try_send(MatterExecutionRequest::Cancel {
            actor,
            operation_id,
        })
    }

    /// Wakes daemon-owned subscription repair.
    ///
    /// # Errors
    ///
    /// Returns busy or unavailable without changing the already durable operation.
    pub fn wake_repair(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
    ) -> Result<(), MatterExecutionError> {
        self.try_send(MatterExecutionRequest::Repair {
            actor,
            operation_id,
        })
    }

    /// Hands non-serializable commissioning input to daemon-owned execution.
    ///
    /// # Errors
    ///
    /// Returns handoff or workflow failures without exposing the input.
    pub async fn commission(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
        input: MatterCommissioningInput,
    ) -> Result<MatterOperation, MatterExecutionError> {
        let (response, received) = tokio::sync::oneshot::channel();
        self.send(MatterExecutionRequest::Commission {
            actor,
            operation_id,
            input,
            response,
        })
        .await?;
        received
            .await
            .map_err(|_| MatterExecutionError::Unavailable)?
    }

    /// Requests one-time sensitive export delivery from daemon-owned execution.
    ///
    /// # Errors
    ///
    /// Returns handoff or workflow failures without exposing export bytes.
    pub async fn export(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
    ) -> Result<MatterSensitiveExport, MatterExecutionError> {
        let (response, received) = tokio::sync::oneshot::channel();
        self.send(MatterExecutionRequest::Export {
            actor,
            operation_id,
            response,
        })
        .await?;
        received
            .await
            .map_err(|_| MatterExecutionError::Unavailable)?
    }

    /// Hands non-serializable restore bytes to daemon-owned execution.
    ///
    /// # Errors
    ///
    /// Returns handoff or workflow failures without exposing the input.
    pub async fn restore(
        &self,
        actor: Actor,
        operation_id: MatterOperationId,
        input: MatterSimulatorRestoreInput,
    ) -> Result<MatterOperation, MatterExecutionError> {
        let (response, received) = tokio::sync::oneshot::channel();
        self.send(MatterExecutionRequest::Restore {
            actor,
            operation_id,
            input,
            response,
        })
        .await?;
        received
            .await
            .map_err(|_| MatterExecutionError::Unavailable)?
    }

    fn try_send(&self, request: MatterExecutionRequest) -> Result<(), MatterExecutionError> {
        self.sender.try_send(request).map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => MatterExecutionError::Busy,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => MatterExecutionError::Unavailable,
        })
    }

    async fn send(&self, request: MatterExecutionRequest) -> Result<(), MatterExecutionError> {
        self.sender
            .send(request)
            .await
            .map_err(|_| MatterExecutionError::Unavailable)
    }
}

impl MatterExecutionWorker {
    /// Executes one queued request. The daemon controls repetition and shutdown.
    ///
    /// # Errors
    ///
    /// Returns an ordinary workflow error to the daemon. Sensitive callers receive
    /// their result through the request reply without exposing its input.
    pub async fn run_next(&mut self) -> Result<bool, MatterExecutionError> {
        let Some(request) = self.receiver.recv().await else {
            return Ok(false);
        };
        match request {
            MatterExecutionRequest::Create {
                actor,
                operation_id,
            } => {
                self.fabric
                    .run_create(&actor, &operation_id, Utc::now())
                    .await?;
            }
            MatterExecutionRequest::Remove {
                actor,
                operation_id,
            } => {
                self.nodes
                    .run_remove_node(&actor, &operation_id, Utc::now())
                    .await?;
            }
            MatterExecutionRequest::Cancel {
                actor,
                operation_id,
            } => {
                let _: MatterCancellationResult = self
                    .nodes
                    .run_cancel_commissioning(&actor, &operation_id, Utc::now())
                    .await?;
            }
            MatterExecutionRequest::Repair {
                actor,
                operation_id,
            } => {
                self.subscriptions
                    .run(&actor, &operation_id, Utc::now())
                    .await?;
            }
            MatterExecutionRequest::Commission {
                actor,
                operation_id,
                input,
                response,
            } => {
                let result = self
                    .nodes
                    .run_commission(&actor, &operation_id, input, Utc::now())
                    .await
                    .map(operation_from_outcome)
                    .map_err(Into::into);
                let _ = response.send(result);
            }
            MatterExecutionRequest::Export {
                actor,
                operation_id,
                response,
            } => {
                let result = self
                    .fabric
                    .run_export(&actor, &operation_id, Utc::now())
                    .await
                    .map(|outcome| match outcome {
                        MatterWorkflowOutcome::Completed { operation, value } => {
                            Ok(MatterSensitiveExport {
                                operation,
                                export: Some(value),
                            })
                        }
                        MatterWorkflowOutcome::Terminal(operation) => Ok(MatterSensitiveExport {
                            operation,
                            export: None,
                        }),
                    })
                    .and_then(std::convert::identity)
                    .map_err(MatterExecutionError::from);
                let _ = response.send(result);
            }
            MatterExecutionRequest::Restore {
                actor,
                operation_id,
                input,
                response,
            } => {
                let result = self
                    .fabric
                    .run_simulator_restore(&actor, &operation_id, input, Utc::now())
                    .await
                    .map(operation_from_outcome)
                    .map_err(Into::into);
                let _ = response.send(result);
            }
        }
        Ok(true)
    }
}

fn operation_from_outcome<T>(outcome: MatterWorkflowOutcome<T>) -> MatterOperation {
    match outcome {
        MatterWorkflowOutcome::Completed { operation, .. }
        | MatterWorkflowOutcome::Terminal(operation) => operation,
    }
}

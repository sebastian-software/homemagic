use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_application::{BoxError, IntegrationSessionPort};
use homemagic_domain::{DeviceId, DeviceRecord};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

/// Adapter-specific execution of one device session.
#[async_trait]
pub trait SessionRunner: Send + Sync {
    /// Runs until cancellation, connection termination, or a session error.
    async fn run(
        &self,
        device: DeviceRecord,
        cancelled: watch::Receiver<bool>,
    ) -> Result<(), BoxError>;
}

struct ManagedSession {
    cancel: watch::Sender<bool>,
    task: JoinHandle<Result<(), BoxError>>,
}

/// Owns at most one integration session task per stable device identity.
#[derive(Clone)]
pub struct ShellySessionSupervisor {
    runner: Arc<dyn SessionRunner>,
    sessions: Arc<Mutex<BTreeMap<DeviceId, ManagedSession>>>,
}

impl ShellySessionSupervisor {
    /// Creates an empty supervisor using the adapter session runner.
    #[must_use]
    pub fn new(runner: Arc<dyn SessionRunner>) -> Self {
        Self {
            runner,
            sessions: Arc::default(),
        }
    }

    /// Returns the current number of owned session tasks.
    pub async fn session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    async fn cancel(managed: ManagedSession) -> Result<(), BoxError> {
        let _ = managed.cancel.send(true);
        managed
            .task
            .await
            .map_err(|error| -> BoxError { Box::new(SessionTaskError(error.to_string())) })?
    }
}

#[derive(Debug, thiserror::Error)]
#[error("managed session task failed: {0}")]
struct SessionTaskError(String);

#[async_trait]
impl IntegrationSessionPort for ShellySessionSupervisor {
    async fn start(&self, device: &DeviceRecord) -> Result<(), BoxError> {
        let mut sessions = self.sessions.lock().await;
        if let Some(previous) = sessions.remove(&device.snapshot.id) {
            Self::cancel(previous).await?;
        }
        let (cancel, cancelled) = watch::channel(false);
        let runner = self.runner.clone();
        let owned_device = device.clone();
        let task = tokio::spawn(async move { runner.run(owned_device, cancelled).await });
        sessions.insert(device.snapshot.id.clone(), ManagedSession { cancel, task });
        Ok(())
    }

    async fn stop(&self, device_id: &DeviceId) -> Result<(), BoxError> {
        let managed = self.sessions.lock().await.remove(device_id);
        if let Some(managed) = managed {
            Self::cancel(managed).await?;
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), BoxError> {
        let managed = {
            let mut sessions = self.sessions.lock().await;
            std::mem::take(&mut *sessions)
                .into_values()
                .collect::<Vec<_>>()
        };
        let mut first_error = None;
        for session in managed {
            if let Err(error) = Self::cancel(session).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use homemagic_application::IntegrationSessionPort;
    use homemagic_domain::{
        Availability, DeviceLifecycle, DeviceSnapshot, DeviceTimestamps, InstallationId,
        IntegrationId,
    };
    use tokio::sync::Notify;

    use super::*;

    struct CountingRunner {
        active: AtomicUsize,
        maximum: AtomicUsize,
        starts: AtomicUsize,
        started: Notify,
    }

    impl CountingRunner {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                maximum: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
                started: Notify::new(),
            }
        }
    }

    #[async_trait]
    impl SessionRunner for CountingRunner {
        async fn run(
            &self,
            _device: DeviceRecord,
            mut cancelled: watch::Receiver<bool>,
        ) -> Result<(), BoxError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.started.notify_waiters();
            while !*cancelled.borrow() {
                if cancelled.changed().await.is_err() {
                    break;
                }
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn device(native_id: &str) -> DeviceRecord {
        let now = Utc::now();
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "shelly", "local");
        let id = DeviceId::from_integration(&integration_id, native_id);
        DeviceRecord {
            installation_id,
            integration_id,
            snapshot: DeviceSnapshot {
                id,
                native_id: native_id.to_owned(),
                integration: "shelly".to_owned(),
                name: "Fixture".to_owned(),
                manufacturer: "Shelly".to_owned(),
                model: "Fixture".to_owned(),
                network: Vec::new(),
                endpoints: Vec::new(),
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            lifecycle: DeviceLifecycle::Enrolled,
            availability: Availability::unknown(now),
            timestamps: DeviceTimestamps::first_seen(now),
            aliases: BTreeSet::new(),
            spaces: BTreeSet::new(),
            capability_descriptors: BTreeMap::new(),
        }
    }

    async fn wait_for_starts(runner: &CountingRunner, expected: usize) {
        while runner.starts.load(Ordering::SeqCst) < expected {
            runner.started.notified().await;
        }
    }

    #[tokio::test]
    async fn replacement_should_never_overlap_same_device() -> Result<(), BoxError> {
        let runner = Arc::new(CountingRunner::new());
        let supervisor = ShellySessionSupervisor::new(runner.clone());
        let device = device("fixture");

        supervisor.start(&device).await?;
        wait_for_starts(&runner, 1).await;
        supervisor.start(&device).await?;
        wait_for_starts(&runner, 2).await;

        assert_eq!(supervisor.session_count().await, 1);
        assert_eq!(runner.maximum.load(Ordering::SeqCst), 1);
        supervisor.shutdown().await?;
        assert_eq!(runner.active.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[tokio::test]
    async fn stop_and_shutdown_should_cancel_owned_tasks() -> Result<(), BoxError> {
        let runner = Arc::new(CountingRunner::new());
        let supervisor = ShellySessionSupervisor::new(runner.clone());
        let first = device("first");
        let second = device("second");
        supervisor.start(&first).await?;
        supervisor.start(&second).await?;
        wait_for_starts(&runner, 2).await;

        supervisor.stop(&first.snapshot.id).await?;
        assert_eq!(supervisor.session_count().await, 1);
        supervisor.shutdown().await?;

        assert_eq!(supervisor.session_count().await, 0);
        assert_eq!(runner.active.load(Ordering::SeqCst), 0);
        Ok(())
    }
}

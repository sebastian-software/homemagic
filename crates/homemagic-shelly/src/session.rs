use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_application::{BoxError, IntegrationSessionPort};
use homemagic_domain::{DeviceId, DeviceRecord};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

/// Bounded exponential reconnect parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackoffPolicy {
    base: std::time::Duration,
    cap: std::time::Duration,
    jitter_ratio: f64,
    stable_reset: std::time::Duration,
}

impl BackoffPolicy {
    /// Creates a validated reconnect policy.
    ///
    /// # Errors
    ///
    /// Rejects zero/unordered durations and jitter outside `0.0..=1.0`.
    pub fn new(
        base: std::time::Duration,
        cap: std::time::Duration,
        jitter_ratio: f64,
        stable_reset: std::time::Duration,
    ) -> Result<Self, BackoffPolicyError> {
        if base.is_zero()
            || cap < base
            || stable_reset.is_zero()
            || !(0.0..=1.0).contains(&jitter_ratio)
        {
            return Err(BackoffPolicyError);
        }
        Ok(Self {
            base,
            cap,
            jitter_ratio,
            stable_reset,
        })
    }

    /// Calculates a deterministic delay for an attempt and jitter sample.
    #[must_use]
    pub fn delay(self, attempt: u32, jitter_sample: f64) -> std::time::Duration {
        let multiplier = 1_u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
        let exponential = self
            .base
            .checked_mul(multiplier)
            .unwrap_or(self.cap)
            .min(self.cap);
        let jitter = exponential.mul_f64(self.jitter_ratio * jitter_sample.clamp(0.0, 1.0));
        exponential.saturating_add(jitter).min(self.cap)
    }

    const fn stable_reset(self) -> std::time::Duration {
        self.stable_reset
    }

    fn next_attempt(self, attempt: u32, connected_for: std::time::Duration) -> u32 {
        if connected_for >= self.stable_reset() {
            0
        } else {
            attempt.saturating_add(1)
        }
    }
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            base: std::time::Duration::from_secs(1),
            cap: std::time::Duration::from_secs(60),
            jitter_ratio: 0.25,
            stable_reset: std::time::Duration::from_secs(300),
        }
    }
}

/// Invalid reconnect bounds.
#[derive(Clone, Copy, Debug, Eq, thiserror::Error, PartialEq)]
#[error("invalid reconnect backoff policy")]
pub struct BackoffPolicyError;

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
    backoff: BackoffPolicy,
}

impl ShellySessionSupervisor {
    /// Creates an empty supervisor using the adapter session runner.
    #[must_use]
    pub fn new(runner: Arc<dyn SessionRunner>) -> Self {
        Self {
            runner,
            sessions: Arc::default(),
            backoff: BackoffPolicy::default(),
        }
    }

    /// Creates a supervisor with explicit reconnect bounds.
    #[must_use]
    pub fn with_backoff(runner: Arc<dyn SessionRunner>, backoff: BackoffPolicy) -> Self {
        Self {
            runner,
            sessions: Arc::default(),
            backoff,
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
        let backoff = self.backoff;
        let task = tokio::spawn(async move {
            supervise_session(runner, owned_device, cancelled, backoff).await
        });
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

async fn supervise_session(
    runner: Arc<dyn SessionRunner>,
    device: DeviceRecord,
    mut cancelled: watch::Receiver<bool>,
    backoff: BackoffPolicy,
) -> Result<(), BoxError> {
    let mut attempt = 0_u32;
    loop {
        if *cancelled.borrow() {
            return Ok(());
        }
        let started = tokio::time::Instant::now();
        let result = runner.run(device.clone(), cancelled.clone()).await;
        if *cancelled.borrow() {
            return Ok(());
        }
        attempt = backoff.next_attempt(attempt, started.elapsed());
        match result {
            Ok(()) => tracing::warn!(
                device_id = %device.snapshot.id,
                attempt,
                "Shelly session ended unexpectedly; scheduling reconnect"
            ),
            Err(error) => tracing::warn!(
                device_id = %device.snapshot.id,
                attempt,
                error = %error,
                "Shelly session failed; scheduling reconnect"
            ),
        }
        let delay = backoff.delay(attempt.saturating_sub(1), rand::random());
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            changed = cancelled.changed() => {
                if changed.is_err() || *cancelled.borrow() {
                    return Ok(());
                }
            }
        }
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

    #[test]
    fn backoff_should_be_deterministic_bounded_and_resettable() {
        let policy = BackoffPolicy::new(
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(10),
            0.25,
            std::time::Duration::from_secs(30),
        )
        .unwrap_or_else(|error| panic!("backoff policy: {error}"));

        assert_eq!(policy.delay(0, 0.0), std::time::Duration::from_secs(1));
        assert_eq!(
            policy.delay(1, 1.0),
            std::time::Duration::from_millis(2_500)
        );
        assert_eq!(policy.delay(20, 1.0), std::time::Duration::from_secs(10));
        assert_eq!(policy.delay(1, -1.0), std::time::Duration::from_secs(2));
        assert_eq!(
            policy.next_attempt(7, std::time::Duration::from_secs(29)),
            8
        );
        assert_eq!(
            policy.next_attempt(7, std::time::Duration::from_secs(30)),
            0
        );
    }

    struct CountingRunner {
        active: AtomicUsize,
        maximum: AtomicUsize,
        starts: AtomicUsize,
        started: Notify,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("fixture connection lost")]
    struct FixtureConnectionLost;

    struct RecoveringRunner {
        starts: AtomicUsize,
        started: Notify,
    }

    #[async_trait]
    impl SessionRunner for RecoveringRunner {
        async fn run(
            &self,
            _device: DeviceRecord,
            mut cancelled: watch::Receiver<bool>,
        ) -> Result<(), BoxError> {
            let starts = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
            self.started.notify_waiters();
            if starts < 3 {
                return Err(Box::new(FixtureConnectionLost));
            }
            while !*cancelled.borrow() {
                if cancelled.changed().await.is_err() {
                    break;
                }
            }
            Ok(())
        }
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

    #[tokio::test]
    async fn failed_session_should_reconnect_until_recovered() -> Result<(), BoxError> {
        let runner = Arc::new(RecoveringRunner {
            starts: AtomicUsize::new(0),
            started: Notify::new(),
        });
        let policy = BackoffPolicy::new(
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(2),
            0.0,
            std::time::Duration::from_secs(1),
        )?;
        let supervisor = ShellySessionSupervisor::with_backoff(runner.clone(), policy);

        supervisor.start(&device("recovering")).await?;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while runner.starts.load(Ordering::SeqCst) < 3 {
                runner.started.notified().await;
            }
        })
        .await
        .map_err(|error| -> BoxError { Box::new(error) })?;

        supervisor.shutdown().await?;
        assert_eq!(runner.starts.load(Ordering::SeqCst), 3);
        Ok(())
    }
}

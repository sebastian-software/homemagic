//! Contract tests for Rust-owned sidecar secrets, cancellation, and flow control.

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use homemagic_matter::{
    CancellationDisposition, EventWindow, ProtocolError, RemoteOperationState, SecretDisposition,
    SecretDriverError, SecretMethod, SecretRecord, SecretRequest, SensitiveBytes, SessionBinding,
    SidecarEventKind, SidecarSecretStore, dispatch_secret_request, event_may_coalesce,
};

#[derive(Default)]
struct FakeSecretStore {
    values: Mutex<BTreeMap<String, (u64, Vec<u8>)>>,
}

#[async_trait]
impl SidecarSecretStore for FakeSecretStore {
    async fn get(&self, handle: &str) -> Result<Option<SecretRecord>, SecretDriverError> {
        let values = self
            .values
            .lock()
            .map_err(|_| SecretDriverError::Unavailable)?;
        Ok(values.get(handle).map(|(revision, value)| SecretRecord {
            revision: *revision,
            value: SensitiveBytes::new(value.clone()),
        }))
    }

    async fn put(&self, handle: &str, value: SensitiveBytes) -> Result<u64, SecretDriverError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretDriverError::Unavailable)?;
        let revision = values.get(handle).map_or(1, |(revision, _)| revision + 1);
        values.insert(handle.into(), (revision, value.expose().to_vec()));
        Ok(revision)
    }

    async fn delete(&self, handle: &str) -> Result<bool, SecretDriverError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretDriverError::Unavailable)?;
        Ok(values.remove(handle).is_some())
    }

    async fn compare_and_swap(
        &self,
        handle: &str,
        expected_revision: u64,
        value: SensitiveBytes,
    ) -> Result<u64, SecretDriverError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| SecretDriverError::Unavailable)?;
        let Some((revision, stored)) = values.get_mut(handle) else {
            return Err(SecretDriverError::Conflict);
        };
        if *revision != expected_revision {
            return Err(SecretDriverError::Conflict);
        }
        *revision += 1;
        *stored = value.expose().to_vec();
        Ok(*revision)
    }
}

fn request(method: SecretMethod, revision: Option<u64>, value: Option<Vec<u8>>) -> SecretRequest {
    SecretRequest {
        session: SessionBinding {
            child_nonce: "child-1".into(),
            session_nonce: "session-1".into(),
        },
        request_id: "secret-request-1".into(),
        method,
        handle: "fabric/main".into(),
        expected_revision: revision,
        value: value.map(SensitiveBytes::new),
    }
}

#[tokio::test]
async fn reverse_secret_driver_should_round_trip_and_enforce_compare_and_swap() {
    let store = FakeSecretStore::default();
    let stored = dispatch_secret_request(
        &store,
        request(SecretMethod::Put, None, Some(b"SECRET-CANARY".to_vec())),
    )
    .await
    .unwrap_or_else(|error| panic!("secret put should pass: {error}"));
    assert_eq!(
        stored.disposition,
        SecretDisposition::Stored { revision: 1 }
    );

    let conflict = dispatch_secret_request(
        &store,
        request(
            SecretMethod::CompareAndSwap,
            Some(7),
            Some(b"replacement".to_vec()),
        ),
    )
    .await
    .unwrap_or_else(|error| panic!("conflict should be a response: {error}"));
    assert_eq!(conflict.disposition, SecretDisposition::Conflict);

    let replaced = dispatch_secret_request(
        &store,
        request(
            SecretMethod::CompareAndSwap,
            Some(1),
            Some(b"replacement".to_vec()),
        ),
    )
    .await
    .unwrap_or_else(|error| panic!("secret replacement should pass: {error}"));
    assert_eq!(
        replaced.disposition,
        SecretDisposition::Stored { revision: 2 }
    );

    let found = dispatch_secret_request(&store, request(SecretMethod::Get, None, None))
        .await
        .unwrap_or_else(|error| panic!("secret get should pass: {error}"));
    assert!(matches!(found.disposition, SecretDisposition::Found { .. }));
    assert!(!format!("{found:?}").contains("replacement"));
}

#[tokio::test]
async fn reverse_secret_driver_should_reject_invalid_method_shapes_without_io() {
    let store = FakeSecretStore::default();
    let invalid = dispatch_secret_request(
        &store,
        request(SecretMethod::Get, None, Some(b"unexpected".to_vec())),
    )
    .await;
    assert!(matches!(invalid, Err(ProtocolError::MalformedFrame)));
    assert!(store.values.lock().is_ok_and(|values| values.is_empty()));
}

#[test]
fn cancellation_should_never_claim_a_remote_mutation_was_cancelled() {
    assert_eq!(
        RemoteOperationState::PreMutation.cancellation(),
        CancellationDisposition::Cancelled
    );
    assert_eq!(
        RemoteOperationState::MutationDispatched.cancellation(),
        CancellationDisposition::TooLate
    );
    assert!(RemoteOperationState::MutationDispatched.disconnect_is_partial());
    assert!(!RemoteOperationState::PreMutation.disconnect_is_partial());
    assert_eq!(
        RemoteOperationState::Uncancellable.cancellation(),
        CancellationDisposition::UnsupportedAtPhase
    );
}

#[test]
fn event_window_should_enforce_contiguous_bounded_acknowledged_delivery() {
    let mut window =
        EventWindow::new(2).unwrap_or_else(|error| panic!("window should be valid: {error}"));
    window
        .receive(1)
        .unwrap_or_else(|error| panic!("first event should pass: {error}"));
    assert!(matches!(
        window.receive(1),
        Err(ProtocolError::DuplicateSequence)
    ));
    assert!(matches!(window.receive(3), Err(ProtocolError::SequenceGap)));
    window
        .receive(2)
        .unwrap_or_else(|error| panic!("second event should pass: {error}"));
    assert!(matches!(
        window.receive(3),
        Err(ProtocolError::EventWindowExhausted)
    ));
    window
        .acknowledge(1)
        .unwrap_or_else(|error| panic!("ack should pass: {error}"));
    window
        .receive(3)
        .unwrap_or_else(|error| panic!("released slot should pass: {error}"));
    assert!(matches!(
        window.acknowledge(4),
        Err(ProtocolError::InvalidAcknowledgement)
    ));

    assert!(event_may_coalesce(SidecarEventKind::AttributeReport));
    assert!(!event_may_coalesce(SidecarEventKind::MatterEvent));
    assert!(!event_may_coalesce(SidecarEventKind::CommissioningProgress));
    assert!(!event_may_coalesce(SidecarEventKind::SubscriptionLost));
}

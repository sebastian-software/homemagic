//! Contract tests for the SDK-neutral private Matter sidecar protocol.

use std::collections::BTreeSet;

use homemagic_matter::{
    HandshakePolicy, Hello, MAX_FRAME_BYTES, PrivatePayload, ProtocolError, ProtocolLimits,
    ProtocolVersion, ResponseDisposition, SessionBinding, SidecarFrame, SidecarMethod,
    SidecarRequest, negotiate, read_json_frame, write_json_frame,
};
use serde_json::json;
use tokio::io::{AsyncWriteExt, duplex};

fn required_methods() -> BTreeSet<SidecarMethod> {
    [
        SidecarMethod::FabricLoad,
        SidecarMethod::NodeCommission,
        SidecarMethod::NodeInventory,
        SidecarMethod::AttributeRead,
        SidecarMethod::CommandInvoke,
        SidecarMethod::SubscriptionOpen,
        SidecarMethod::OperationCancel,
        SidecarMethod::HealthCheck,
    ]
    .into_iter()
    .collect()
}

fn hello() -> Hello {
    Hello {
        protocol: ProtocolVersion::CURRENT,
        matter_js_revision: "b539372ff41fea24344760d69172508e9df931a2".into(),
        node_version: "v24.18.0".into(),
        methods: required_methods(),
        event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
        child_nonce: "child-nonce-1".into(),
    }
}

#[test]
fn handshake_should_pin_runtime_capabilities_and_limits() {
    let methods = required_methods();
    let events = BTreeSet::new();
    let policy = HandshakePolicy {
        minimum_minor: 0,
        matter_js_revision: "b539372ff41fea24344760d69172508e9df931a2",
        node_version: "v24.18.0",
        required_methods: &methods,
        required_event_kinds: &events,
        limits: ProtocolLimits {
            event_window: 8,
            ..ProtocolLimits::default()
        },
    };

    let accept = negotiate(
        &hello(),
        &policy,
        "installation-1".into(),
        "session-nonce-1".into(),
    );
    let accept = accept.unwrap_or_else(|error| panic!("handshake should pass: {error}"));
    assert_eq!(accept.protocol, ProtocolVersion::CURRENT);
    assert_eq!(accept.child_nonce, "child-nonce-1");
    assert_eq!(accept.limits.event_window, 8);
}

#[test]
fn handshake_should_fail_closed_for_downgrade_runtime_and_capability_mismatch() {
    let methods = required_methods();
    let events = BTreeSet::new();
    let policy = HandshakePolicy {
        minimum_minor: 0,
        matter_js_revision: "b539372ff41fea24344760d69172508e9df931a2",
        node_version: "v24.18.0",
        required_methods: &methods,
        required_event_kinds: &events,
        limits: ProtocolLimits::default(),
    };

    let mut candidate = hello();
    candidate.protocol.major = 2;
    assert!(matches!(
        negotiate(
            &candidate,
            &policy,
            "installation-1".into(),
            "session-1".into()
        ),
        Err(ProtocolError::IncompatibleMajor { offered: 2 })
    ));

    candidate = hello();
    candidate.node_version = "v24.19.0".into();
    assert!(matches!(
        negotiate(
            &candidate,
            &policy,
            "installation-1".into(),
            "session-1".into()
        ),
        Err(ProtocolError::UnexpectedRuntime)
    ));

    candidate = hello();
    candidate.methods.remove(&SidecarMethod::OperationCancel);
    assert!(matches!(
        negotiate(
            &candidate,
            &policy,
            "installation-1".into(),
            "session-1".into()
        ),
        Err(ProtocolError::MissingCapability)
    ));

    candidate = hello();
    let downgrade_policy = HandshakePolicy {
        minimum_minor: 1,
        ..policy
    };
    assert!(matches!(
        negotiate(
            &candidate,
            &downgrade_policy,
            "installation-1".into(),
            "session-1".into()
        ),
        Err(ProtocolError::Downgrade { offered: 0 })
    ));
}

#[tokio::test]
async fn framing_should_round_trip_and_reject_malformed_or_oversized_frames() {
    let (mut writer, mut reader) = duplex(MAX_FRAME_BYTES + 16);
    let frame = SidecarFrame::Request(SidecarRequest {
        session: SessionBinding {
            child_nonce: "child-1".into(),
            session_nonce: "session-1".into(),
        },
        request_id: "request-1".into(),
        method: SidecarMethod::HealthCheck,
        deadline_ms: 1_000,
        idempotency_key: "health-1".into(),
        body: PrivatePayload::new(json!({"probe": true})),
    });

    write_json_frame(&mut writer, &frame, MAX_FRAME_BYTES)
        .await
        .unwrap_or_else(|error| panic!("frame write should pass: {error}"));
    let decoded: SidecarFrame = read_json_frame(&mut reader, MAX_FRAME_BYTES)
        .await
        .unwrap_or_else(|error| panic!("frame read should pass: {error}"));
    assert_eq!(decoded, frame);

    let (mut writer, mut reader) = duplex(32);
    writer
        .write_u32(1_048_577)
        .await
        .unwrap_or_else(|error| panic!("length write should pass: {error}"));
    let oversized = read_json_frame::<_, SidecarFrame>(&mut reader, MAX_FRAME_BYTES).await;
    assert!(matches!(oversized, Err(ProtocolError::OversizedFrame)));

    let (mut writer, mut reader) = duplex(32);
    writer
        .write_u32(1)
        .await
        .unwrap_or_else(|error| panic!("length write should pass: {error}"));
    writer
        .write_all(b"{")
        .await
        .unwrap_or_else(|error| panic!("payload write should pass: {error}"));
    let malformed = read_json_frame::<_, SidecarFrame>(&mut reader, MAX_FRAME_BYTES).await;
    assert!(matches!(malformed, Err(ProtocolError::MalformedFrame)));
}

#[test]
fn request_and_partial_payloads_should_be_redacted_in_diagnostics() {
    let request = SidecarRequest {
        session: SessionBinding {
            child_nonce: "child-1".into(),
            session_nonce: "session-1".into(),
        },
        request_id: "request-1".into(),
        method: SidecarMethod::NodeCommission,
        deadline_ms: 30_000,
        idempotency_key: "commission-1".into(),
        body: PrivatePayload::new(json!({"setup_code": "SECRET-CANARY"})),
    };
    let partial = ResponseDisposition::Partial {
        phase: "fabric_installed".into(),
        body: PrivatePayload::new(json!({"private_key": "SECRET-CANARY"})),
    };

    let diagnostics = format!("{request:?} {partial:?}");
    assert!(!diagnostics.contains("SECRET-CANARY"));
    assert!(diagnostics.contains("[REDACTED]"));
}

//! Executable private-protocol fixture for supervisor fault tests.

use std::collections::BTreeSet;

use homemagic_matter::{
    Hello, MAX_FRAME_BYTES, PrivatePayload, ProtocolLimits, ProtocolVersion, ResponseDisposition,
    SecretMethod, SecretRequest, SessionBinding, SidecarEvent, SidecarEventKind, SidecarFailure,
    SidecarFrame, SidecarMethod, SidecarResponse, read_json_frame, write_json_frame,
};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite, stdin, stdout};

async fn exchange_control<R, W>(reader: &mut R, writer: &mut W, session: SessionBinding) -> bool
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let secret = SidecarFrame::SecretRequest(SecretRequest {
        session: session.clone(),
        request_id: "reverse-secret-1".into(),
        method: SecretMethod::Get,
        handle: "fabric/main".into(),
        expected_revision: None,
        value: None,
    });
    if write_json_frame(writer, &secret, MAX_FRAME_BYTES)
        .await
        .is_err()
    {
        return false;
    }
    let Ok(SidecarFrame::SecretResponse(_)) = read_json_frame(reader, MAX_FRAME_BYTES).await else {
        return false;
    };
    let event = SidecarFrame::Event(SidecarEvent {
        session,
        sequence: 1,
        subscription_id: "subscription-1".into(),
        operation_id: None,
        kind: SidecarEventKind::AttributeReport,
        body: PrivatePayload::new(json!({"value": true})),
    });
    if write_json_frame(writer, &event, MAX_FRAME_BYTES)
        .await
        .is_err()
    {
        return false;
    }
    matches!(
        read_json_frame(reader, MAX_FRAME_BYTES).await,
        Ok(SidecarFrame::EventAck(_))
    )
}

#[tokio::main]
async fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
    let mut reader = stdin();
    let mut writer = stdout();
    if mode == "startup-hang" {
        std::future::pending::<()>().await;
    }
    let protocol = if mode == "bad-version" {
        ProtocolVersion { major: 2, minor: 0 }
    } else {
        ProtocolVersion::CURRENT
    };
    let hello = Hello {
        protocol,
        matter_js_revision: "fixture-revision".into(),
        node_version: "fixture-node".into(),
        methods: [SidecarMethod::HealthCheck, SidecarMethod::ProcessDrain]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        event_kinds: BTreeSet::new(),
        limits: ProtocolLimits::default(),
        child_nonce: "fixture-child".into(),
    };
    if write_json_frame(&mut writer, &hello, MAX_FRAME_BYTES)
        .await
        .is_err()
    {
        return;
    }
    let Ok(SidecarFrame::Accept(_)) = read_json_frame(&mut reader, MAX_FRAME_BYTES).await else {
        return;
    };
    if mode == "crash" {
        return;
    }

    loop {
        let Ok(SidecarFrame::Request(request)) =
            read_json_frame(&mut reader, MAX_FRAME_BYTES).await
        else {
            return;
        };
        if mode == "hang" || (mode == "drain-hang" && request.method == SidecarMethod::ProcessDrain)
        {
            std::future::pending::<()>().await;
        }
        let drain = request.method == SidecarMethod::ProcessDrain;
        if mode == "control"
            && request.method == SidecarMethod::HealthCheck
            && !exchange_control(&mut reader, &mut writer, request.session.clone()).await
        {
            return;
        }
        let disposition = if request.method == SidecarMethod::HealthCheck || drain {
            ResponseDisposition::Result {
                body: PrivatePayload::new(json!({"healthy": true})),
            }
        } else {
            ResponseDisposition::Error {
                error: SidecarFailure {
                    code: "unsupported".into(),
                    retryable: false,
                },
            }
        };
        let mut session = request.session;
        if mode == "wrong-session" {
            session.session_nonce = "wrong-session".into();
        }
        let response = SidecarFrame::Response(SidecarResponse {
            session,
            request_id: request.request_id,
            disposition,
        });
        if write_json_frame(&mut writer, &response, MAX_FRAME_BYTES)
            .await
            .is_err()
        {
            return;
        }
        if drain {
            return;
        }
    }
}

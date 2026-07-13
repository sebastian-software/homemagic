//! Executable private-protocol fixture for supervisor fault tests.

use std::collections::BTreeSet;

use homemagic_matter::{
    Hello, MAX_FRAME_BYTES, PrivatePayload, ProtocolLimits, ProtocolVersion, ResponseDisposition,
    SidecarFailure, SidecarFrame, SidecarMethod, SidecarResponse, read_json_frame,
    write_json_frame,
};
use serde_json::json;
use tokio::io::{stdin, stdout};

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

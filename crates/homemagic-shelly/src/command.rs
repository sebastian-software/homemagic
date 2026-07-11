#![allow(
    dead_code,
    reason = "private typed mapping is consumed by the immediately following transport slice"
)]

use homemagic_domain::{
    CommandEnvelope, CommandErrorCode, CommandFailure, CommandPayload, LevelCommand, OnOffCommand,
    PositionCommand,
};
use serde_json::{Map, Value, json};

const ORIGIN_TAG: &str = "homemagic";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShellyRpcCall {
    pub method: &'static str,
    pub params: Map<String, Value>,
}

pub(crate) fn map_command(command: &CommandEnvelope) -> Result<ShellyRpcCall, CommandFailure> {
    let component = Component::parse(command.endpoint_id.as_str())?;
    match (&command.payload, component.kind) {
        (CommandPayload::OnOff(action), ComponentKind::Switch | ComponentKind::Light) => {
            map_on_off(*action, component)
        }
        (CommandPayload::Level(level), ComponentKind::Light) => Ok(map_level(*level, component)),
        (CommandPayload::Position(position), ComponentKind::Cover) => {
            map_position(*position, component)
        }
        _ => Err(failure(CommandErrorCode::CapabilityMismatch)),
    }
}

fn map_on_off(action: OnOffCommand, component: Component) -> Result<ShellyRpcCall, CommandFailure> {
    let namespace = match component.kind {
        ComponentKind::Switch => "Switch",
        ComponentKind::Light => "Light",
        ComponentKind::Cover => return Err(failure(CommandErrorCode::CapabilityMismatch)),
    };
    let (operation, params) = match action {
        OnOffCommand::Set { on } => (
            "Set",
            json!({"id": component.id, "on": on, "tag": ORIGIN_TAG}),
        ),
        OnOffCommand::Toggle => ("Toggle", json!({"id": component.id, "tag": ORIGIN_TAG})),
    };
    call(namespace, operation, params)
}

fn map_level(level: LevelCommand, component: Component) -> ShellyRpcCall {
    let mut params = Map::from_iter([
        ("id".to_owned(), json!(component.id)),
        ("on".to_owned(), json!(level.percent > 0)),
        ("brightness".to_owned(), json!(level.percent)),
        ("tag".to_owned(), json!(ORIGIN_TAG)),
    ]);
    if let Some(milliseconds) = level.transition_ms {
        params.insert(
            "transition_duration".to_owned(),
            json!(f64::from(milliseconds) / 1_000.0),
        );
    }
    ShellyRpcCall {
        method: "Light.Set",
        params,
    }
}

fn map_position(
    position: PositionCommand,
    component: Component,
) -> Result<ShellyRpcCall, CommandFailure> {
    let (operation, params) = match position {
        PositionCommand::Open => ("Open", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::Close => ("Close", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::Stop => ("Stop", json!({"id": component.id, "tag": ORIGIN_TAG})),
        PositionCommand::GoTo { percent } => (
            "GoToPosition",
            json!({"id": component.id, "pos": percent, "tag": ORIGIN_TAG}),
        ),
    };
    call("Cover", operation, params)
}

fn call(namespace: &str, operation: &str, params: Value) -> Result<ShellyRpcCall, CommandFailure> {
    let Value::Object(params) = params else {
        return Err(failure(CommandErrorCode::AdapterRejected));
    };
    let method = match (namespace, operation) {
        ("Switch", "Set") => "Switch.Set",
        ("Switch", "Toggle") => "Switch.Toggle",
        ("Light", "Set") => "Light.Set",
        ("Light", "Toggle") => "Light.Toggle",
        ("Cover", "Open") => "Cover.Open",
        ("Cover", "Close") => "Cover.Close",
        ("Cover", "Stop") => "Cover.Stop",
        ("Cover", "GoToPosition") => "Cover.GoToPosition",
        _ => return Err(failure(CommandErrorCode::CapabilityMismatch)),
    };
    Ok(ShellyRpcCall { method, params })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComponentKind {
    Switch,
    Light,
    Cover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Component {
    kind: ComponentKind,
    id: u16,
}

impl Component {
    fn parse(value: &str) -> Result<Self, CommandFailure> {
        let Some((kind, id)) = value.split_once(':') else {
            return Err(failure(CommandErrorCode::CapabilityMismatch));
        };
        let kind = match kind {
            "switch" => ComponentKind::Switch,
            "light" => ComponentKind::Light,
            "cover" => ComponentKind::Cover,
            _ => return Err(failure(CommandErrorCode::CapabilityMismatch)),
        };
        let id = id
            .parse::<u16>()
            .map_err(|_| failure(CommandErrorCode::CapabilityMismatch))?;
        Ok(Self { kind, id })
    }
}

pub(crate) fn normalize_rpc_error(code: i32, message: &str) -> CommandFailure {
    let message = message.to_ascii_lowercase();
    let code = if message.contains("overtemp") || message.contains("temperature") {
        CommandErrorCode::Overtemperature
    } else if message.contains("obstruction") {
        CommandErrorCode::ObstructionDetected
    } else if message.contains("safety")
        || message.contains("overpower")
        || message.contains("overcurrent")
        || message.contains("overvoltage")
        || message.contains("undervoltage")
    {
        CommandErrorCode::ProtectionActive
    } else if code == -109 || message.contains("calibrat") || message.contains("position unknown") {
        CommandErrorCode::UnsupportedConstraint
    } else {
        CommandErrorCode::AdapterRejected
    };
    failure(code)
}

fn failure(code: CommandErrorCode) -> CommandFailure {
    CommandFailure { code, detail: None }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};
    use homemagic_domain::{
        ActorId, CapabilityDescriptor, CommandId, CorrelationId, DeviceId, EndpointId,
        IdempotencyKey, IntegrationId, RiskClass,
    };

    use super::*;

    fn envelope(endpoint: &str, payload: CommandPayload) -> CommandEnvelope {
        let installation = homemagic_domain::InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "shelly", "local");
        let now = Utc::now();
        CommandEnvelope {
            id: CommandId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::from_integration(&integration, "fixture"),
            endpoint_id: EndpointId::new(endpoint),
            capability: CapabilityDescriptor::new(
                payload.schema().trim_end_matches(".v1"),
                1,
                RiskClass::Comfort,
            )
            .unwrap_or_else(|error| panic!("descriptor: {error}")),
            payload,
            idempotency_key: IdempotencyKey::new("fixture")
                .unwrap_or_else(|error| panic!("key: {error}")),
            deadline: now + TimeDelta::seconds(10),
            expected: None,
            dry_run: false,
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            received_at: now,
        }
    }

    #[test]
    fn should_map_switch_and_light_commands_without_raw_input() {
        let switch = map_command(&envelope(
            "switch:2",
            CommandPayload::OnOff(OnOffCommand::Set { on: true }),
        ))
        .unwrap_or_else(|error| panic!("switch mapping: {error:?}"));
        let toggle = map_command(&envelope(
            "light:0",
            CommandPayload::OnOff(OnOffCommand::Toggle),
        ))
        .unwrap_or_else(|error| panic!("toggle mapping: {error:?}"));
        let level = map_command(&envelope(
            "light:0",
            CommandPayload::Level(LevelCommand {
                percent: 42,
                transition_ms: Some(1_500),
            }),
        ))
        .unwrap_or_else(|error| panic!("level mapping: {error:?}"));

        assert_eq!(switch.method, "Switch.Set");
        assert_eq!(switch.params["id"], json!(2));
        assert_eq!(toggle.method, "Light.Toggle");
        assert_eq!(level.params["brightness"], json!(42));
        assert_eq!(level.params["transition_duration"], json!(1.5));
    }

    #[test]
    fn should_map_every_cover_operation() {
        let cases = [
            (PositionCommand::Open, "Cover.Open"),
            (PositionCommand::Close, "Cover.Close"),
            (PositionCommand::Stop, "Cover.Stop"),
            (PositionCommand::GoTo { percent: 73 }, "Cover.GoToPosition"),
        ];
        for (position, expected) in cases {
            let call = map_command(&envelope("cover:0", CommandPayload::Position(position)))
                .unwrap_or_else(|error| panic!("cover mapping: {error:?}"));
            assert_eq!(call.method, expected);
        }
    }

    #[test]
    fn should_reject_component_capability_mismatch() {
        let result = map_command(&envelope(
            "switch:0",
            CommandPayload::Position(PositionCommand::Open),
        ));

        assert_eq!(
            result,
            Err(CommandFailure {
                code: CommandErrorCode::CapabilityMismatch,
                detail: None,
            })
        );
    }

    #[test]
    fn should_normalize_safety_and_rpc_errors() {
        assert_eq!(
            normalize_rpc_error(-109, "Current position unknown").code,
            CommandErrorCode::UnsupportedConstraint
        );
        assert_eq!(
            normalize_rpc_error(-1, "obstruction detected").code,
            CommandErrorCode::ObstructionDetected
        );
        assert_eq!(
            normalize_rpc_error(-1, "overtemp").code,
            CommandErrorCode::Overtemperature
        );
        assert_eq!(
            normalize_rpc_error(-1, "safety switch engaged").code,
            CommandErrorCode::ProtectionActive
        );
    }
}

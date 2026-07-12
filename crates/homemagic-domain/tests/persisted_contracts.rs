//! Serialization contracts for domain values persisted by EPIC-001/002 storage.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use chrono::Utc;
use homemagic_domain::{
    ActorId, AuditId, AutomationAction, AutomationApprovalId, AutomationApprovalRecord,
    AutomationApprovalRequirement, AutomationApprovalState, AutomationCatchUp,
    AutomationContentHash, AutomationDocument, AutomationExecutionPlan, AutomationOccurrence,
    AutomationOccurrenceId, AutomationOccurrenceState, AutomationPlanNode, AutomationPlanNodeId,
    AutomationPlanNodeKind, AutomationPlanSchema, AutomationRegistryRevision,
    AutomationResourceBudget, AutomationRun, AutomationRunId, AutomationRunMode,
    AutomationRunState, AutomationSafetyProfile, AutomationSafetyRequirement,
    AutomationSelfTriggerPolicy, AutomationTimer, AutomationTimerId, AutomationTimerKind,
    AutomationTimerState, AutomationTraceId, AutomationTraceKind, AutomationTraceStep,
    AutomationValidationCode, AutomationValidationError, AutomationVersionState, AvailabilityState,
    CapabilityDescriptor, CapabilityDescriptorError, CapabilityObservation, CausationMetadata,
    CommandAggregate, CommandAuditRecord, CommandEnvelope, CommandId, CommandPayload, CommandState,
    CommandTransitionError, CorrelationId, DeviceId, DeviceLifecycle, DeviceRecord, DeviceSnapshot,
    DomainEvent, DomainEventKind, EndpointId, EventId, IdempotencyKey, InstallationId,
    IntegrationId, LifecycleTransitionError, LifecycleTrigger, ObservationMergeError,
    ObservationSource, ObservationSourceKind, ObservedValue, OnOffCommand, RepairKind,
    RepairRecord, RepairTransitionError, RiskClass, canonical_automation_hash,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

fn round_trip<T>(value: &T) -> serde_json::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_vec(value)?;
    let decoded = serde_json::from_slice::<T>(&encoded)?;
    assert_eq!(&decoded, value);
    Ok(())
}

#[test]
fn persisted_domain_contracts_should_round_trip() -> Result<(), Box<dyn Error>> {
    let now = Utc::now();
    let installation = InstallationId::new();
    let integration = IntegrationId::from_native(&installation, "test", "local");
    let device_id = DeviceId::from_integration(&integration, "native");
    let descriptor = CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?;
    let record = DeviceRecord::candidate(
        installation.clone(),
        integration.clone(),
        DeviceSnapshot {
            id: device_id.clone(),
            native_id: "native".to_owned(),
            integration: "test".to_owned(),
            name: "Fixture".to_owned(),
            manufacturer: "Test".to_owned(),
            model: "Fixture".to_owned(),
            network: Vec::new(),
            endpoints: Vec::new(),
            observed_at: now,
            vendor_data: BTreeMap::new(),
        },
        now,
    );
    let observation = CapabilityObservation {
        device_id: device_id.clone(),
        endpoint_id: EndpointId::new("switch:0"),
        capability: descriptor,
        values: BTreeMap::from([(
            "on".to_owned(),
            ObservedValue {
                value: json!(true),
                observed_at: now,
            },
        )]),
        received_at: now,
        source: ObservationSource {
            integration_id: integration,
            kind: ObservationSourceKind::FullStatus,
            sequence: Some(1),
        },
    };
    let event = DomainEvent {
        id: EventId::new(),
        device_id: Some(device_id.clone()),
        occurred_at: now,
        causation: CausationMetadata {
            correlation_id: CorrelationId::new(),
            causation_event_id: None,
            actor: None,
            automation: None,
        },
        kind: DomainEventKind::AvailabilityChanged {
            from: AvailabilityState::Unknown,
            to: AvailabilityState::Online,
            reason: None,
        },
    };
    let repair = RepairRecord::new(
        Some(device_id),
        RepairKind::CredentialsMissing,
        "Configure device credentials",
        now,
    );
    let command = CommandAggregate::received(CommandEnvelope {
        id: CommandId::new(),
        actor_id: ActorId::new(),
        device_id: record.snapshot.id.clone(),
        endpoint_id: EndpointId::new("switch:0"),
        capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?,
        payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
        idempotency_key: IdempotencyKey::new("round-trip")?,
        deadline: now + chrono::TimeDelta::seconds(30),
        expected: None,
        dry_run: false,
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
        automation_causation: None,
        received_at: now,
    });

    round_trip(&record)?;
    round_trip(&observation)?;
    round_trip(&event)?;
    round_trip(&repair)?;
    round_trip(&command)?;

    let transition = DomainEventKind::LifecycleChanged {
        from: homemagic_domain::DeviceLifecycle::Candidate,
        to: homemagic_domain::DeviceLifecycle::Enrolled,
        trigger: LifecycleTrigger::Enroll,
    };
    round_trip(&transition)?;
    Ok(())
}

#[test]
fn command_audit_contract_should_round_trip() -> serde_json::Result<()> {
    let audit = CommandAuditRecord {
        id: AuditId::new(),
        command_id: CommandId::new(),
        sequence: 0,
        from: None,
        to: CommandState::Received,
        actor_id: ActorId::new(),
        policy: None,
        failure: None,
        acknowledgement: None,
        confirmation: None,
        correlation_id: CorrelationId::new(),
        causation_event_id: None,
        occurred_at: Utc::now(),
    };

    round_trip(&audit)
}

#[test]
fn public_errors_should_serialize_without_runtime_context() -> serde_json::Result<()> {
    round_trip(&CapabilityDescriptorError::InvalidVersion)?;
    round_trip(&LifecycleTransitionError {
        current: DeviceLifecycle::Removed,
        trigger: LifecycleTrigger::MarkStale,
    })?;
    round_trip(&ObservationMergeError::TargetMismatch)?;
    round_trip(&RepairTransitionError::AlreadyClosed)?;
    round_trip(&CommandTransitionError {
        from: CommandState::Confirmed,
        to: CommandState::Dispatched,
    })?;
    Ok(())
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one integration fixture covers every persisted automation boundary"
)]
fn automation_persisted_contracts_should_round_trip() -> Result<(), Box<dyn Error>> {
    let document: AutomationDocument = serde_json::from_str(include_str!(
        "../../../docs/api/examples/automation-document-v1.json"
    ))?;
    let document_hash = canonical_automation_hash(&document)?;
    let plan_hash = AutomationContentHash::new(
        "1111111111111111111111111111111111111111111111111111111111111111",
    )?;
    let node_id = AutomationPlanNodeId(0);
    let plan = AutomationExecutionPlan {
        schema: AutomationPlanSchema::V1,
        automation_id: document.id.clone(),
        automation_version: document.version,
        document_hash: document_hash.clone(),
        plan_hash: plan_hash.clone(),
        registry_revision: AutomationRegistryRevision(7),
        variables: document.variables.clone(),
        triggers: Vec::new(),
        condition: None,
        run_mode: document.run_mode,
        self_trigger: document.self_trigger,
        entry: node_id,
        nodes: vec![AutomationPlanNode {
            id: node_id,
            order: 0,
            kind: AutomationPlanNodeKind::Complete,
        }],
        safety_profiles: BTreeSet::from([AutomationSafetyProfile::Comfort]),
        safety_requirements: BTreeSet::from([AutomationSafetyRequirement::FreshState]),
        approval: AutomationApprovalRequirement::ActivationGrant,
        budget: AutomationResourceBudget::default(),
    };
    let occurrence_id = AutomationOccurrenceId::new();
    let correlation_id = CorrelationId::new();
    let now = Utc::now();
    let occurrence = AutomationOccurrence {
        id: occurrence_id.clone(),
        automation_id: document.id.clone(),
        version: document.version,
        occurred_at: now,
        window_ends_at: now + chrono::TimeDelta::seconds(60),
        state: AutomationOccurrenceState::Accepted,
        event_cursor: Some(4),
        correlation_id: correlation_id.clone(),
        causation_event_id: None,
        catch_up: Some(AutomationCatchUp {
            missed_occurrence_id: AutomationOccurrenceId::new(),
            requested_by: document.provenance.author_id.clone(),
            idempotency_key: IdempotencyKey::new("persisted-catch-up")?,
            requested_at: now,
        }),
    };
    let run_id = AutomationRunId::new();
    let run = AutomationRun {
        id: run_id.clone(),
        automation_id: document.id.clone(),
        version: document.version,
        occurrence_id,
        actor_id: document.provenance.author_id.clone(),
        state: AutomationRunState::Waiting,
        revision: 2,
        node_id: Some(node_id),
        variables: BTreeMap::new(),
        command_ids: vec![CommandId::new()],
        command_attempt: None,
        condition_durations: Vec::new(),
        continuations: Vec::new(),
        correlation_id: correlation_id.clone(),
        causation_event_id: None,
        created_at: now,
        updated_at: now,
    };
    let timer = AutomationTimer {
        id: AutomationTimerId::new(),
        run_id: run_id.clone(),
        node_id,
        kind: AutomationTimerKind::Delay,
        ready_at: now + chrono::TimeDelta::seconds(1),
        state: AutomationTimerState::Pending,
    };
    let trace = AutomationTraceStep {
        id: AutomationTraceId::new(),
        run_id,
        sequence: 0,
        node_id: Some(node_id),
        kind: AutomationTraceKind::Timer,
        details: BTreeMap::new(),
        occurred_at: now,
        correlation_id,
        causation_event_id: None,
    };
    let approval = AutomationApprovalRecord {
        id: AutomationApprovalId::new(),
        automation_id: document.id.clone(),
        version: document.version,
        document_hash,
        plan_hash,
        actor_id: document.provenance.author_id.clone(),
        state: AutomationApprovalState::Approved,
        rationale: Some("Reviewed".to_owned()),
        decided_at: now,
    };
    let validation = AutomationValidationError {
        code: AutomationValidationCode::ReferenceMissing,
        path: "/actions/0/target".to_owned(),
        reason: "target is absent".to_owned(),
        remediation: Some("select one device".to_owned()),
        reference: None,
    };

    round_trip(&document)?;
    round_trip(&plan)?;
    round_trip(&occurrence)?;
    round_trip(&run)?;
    round_trip(&timer)?;
    round_trip(&trace)?;
    round_trip(&approval)?;
    round_trip(&validation)?;
    round_trip(&AutomationVersionState::Ready)?;
    round_trip(&AutomationRunMode::Queued { capacity: 16 })?;
    round_trip(&AutomationSelfTriggerPolicy::SuppressSameVersion)?;
    round_trip(&AutomationAction::Delay { duration_ms: 1 })?;
    Ok(())
}

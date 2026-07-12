//! Durable normalized-event subscription for active automation versions.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use homemagic_domain::{
    AutomationOccurrence, AutomationOccurrenceId, AutomationOccurrenceState,
    AutomationSelfTriggerPolicy, DomainEvent, DomainEventKind, ResolvedAutomationTarget,
    ResolvedAutomationTrigger,
};
use thiserror::Error;

use crate::{
    ActiveAutomationVersion, AutomationRepository, BoxError, Clock, CursorEvent,
    FoundationRepository,
};

const MAX_EVENT_PAGE: usize = 1_000;
const MAX_ACTIVE_AUTOMATIONS: usize = 1_000;

/// Failure of one bounded durable automation-event pass.
#[derive(Debug, Error)]
pub enum AutomationEventProcessorError {
    /// A durable repository operation failed.
    #[error("automation event processor repository operation failed during {operation}")]
    Repository {
        /// Stable operation name without data-bearing context.
        operation: &'static str,
        /// Adapter-specific failure.
        #[source]
        source: BoxError,
    },
    /// The consumer cursor fell behind retained event history.
    #[error("automation event cursor {requested} expired; earliest retained cursor is {earliest}")]
    CursorExpired {
        /// Last completely handled cursor.
        requested: u64,
        /// Earliest event still retained.
        earliest: u64,
    },
}

/// Summary of one bounded event-consumer pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutomationEventProcessorTick {
    /// Durable events completely examined.
    pub events: u64,
    /// Matching occurrences inserted or confirmed idempotently.
    pub occurrences: u64,
    /// Matching occurrences suppressed by self-trigger policy.
    pub suppressed: u64,
    /// Highest completely handled event cursor.
    pub cursor: u64,
}

/// Cursor-checkpointed subscriber that materializes event occurrences only for
/// exact active automation versions. It never interprets plans or dispatches.
#[derive(Clone)]
pub struct AutomationEventProcessor {
    automations: Arc<dyn AutomationRepository>,
    foundation: Arc<dyn FoundationRepository>,
    clock: Arc<dyn Clock>,
}

impl AutomationEventProcessor {
    /// Creates a processor from its two durable repository ports and clock.
    #[must_use]
    pub fn new(
        automations: Arc<dyn AutomationRepository>,
        foundation: Arc<dyn FoundationRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            automations,
            foundation,
            clock,
        }
    }

    /// Materializes at most `limit` cursor-ordered events and checkpoints each
    /// event only after every matching occurrence is durable.
    ///
    /// # Errors
    ///
    /// Returns typed retention-gap or repository failures. A failed event is
    /// retried from the prior checkpoint and stable occurrence IDs make replay
    /// idempotent.
    pub async fn process(
        &self,
        limit: usize,
    ) -> Result<AutomationEventProcessorTick, AutomationEventProcessorError> {
        let mut checkpoint = self
            .automations
            .automation_event_cursor()
            .await
            .map_err(|source| repository_error("load_cursor", source))?;
        let page = self
            .foundation
            .events_after(checkpoint.cursor, limit.clamp(1, MAX_EVENT_PAGE))
            .await
            .map_err(|source| repository_error("load_events", source))?;
        if let Some(earliest) = page.earliest_cursor
            && checkpoint.cursor.saturating_add(1) < earliest
        {
            return Err(AutomationEventProcessorError::CursorExpired {
                requested: checkpoint.cursor,
                earliest,
            });
        }
        let active = self
            .automations
            .active_automation_versions(MAX_ACTIVE_AUTOMATIONS)
            .await
            .map_err(|source| repository_error("load_active_versions", source))?;
        let mut result = AutomationEventProcessorTick {
            cursor: checkpoint.cursor,
            ..AutomationEventProcessorTick::default()
        };
        for event in page.events {
            self.materialize_event(&active, &event, &mut result).await?;
            checkpoint = self
                .automations
                .advance_automation_event_cursor(
                    checkpoint.revision,
                    event.cursor,
                    self.clock.now(),
                )
                .await
                .map_err(|source| repository_error("advance_cursor", source))?;
            result.events = result.events.saturating_add(1);
            result.cursor = checkpoint.cursor;
        }
        Ok(result)
    }

    async fn materialize_event(
        &self,
        active: &[ActiveAutomationVersion],
        event: &CursorEvent,
        result: &mut AutomationEventProcessorTick,
    ) -> Result<(), AutomationEventProcessorError> {
        for automation in active {
            if !automation
                .version
                .plan
                .triggers
                .iter()
                .any(|trigger| trigger_matches(trigger, &event.event))
            {
                continue;
            }
            let suppressed = self_suppressed(automation, &event.event);
            let id = AutomationOccurrenceId::from_key(
                &automation.identity.id,
                automation.version.document.version.get(),
                &format!("event:{}", event.cursor),
            );
            self.automations
                .create_automation_occurrence(AutomationOccurrence {
                    id,
                    automation_id: automation.identity.id.clone(),
                    version: automation.version.document.version,
                    occurred_at: event.event.occurred_at,
                    window_ends_at: DateTime::<Utc>::MAX_UTC,
                    state: if suppressed {
                        AutomationOccurrenceState::Suppressed
                    } else {
                        AutomationOccurrenceState::Scheduled
                    },
                    event_cursor: Some(event.cursor),
                    correlation_id: event.event.causation.correlation_id.clone(),
                    causation_event_id: Some(event.event.id.clone()),
                    catch_up: None,
                })
                .await
                .map_err(|source| repository_error("create_occurrence", source))?;
            result.occurrences = result.occurrences.saturating_add(1);
            if suppressed {
                result.suppressed = result.suppressed.saturating_add(1);
            }
        }
        Ok(())
    }
}

fn repository_error(operation: &'static str, source: BoxError) -> AutomationEventProcessorError {
    AutomationEventProcessorError::Repository { operation, source }
}

fn self_suppressed(automation: &ActiveAutomationVersion, event: &DomainEvent) -> bool {
    let Some(cause) = &event.causation.automation else {
        return false;
    };
    if cause.automation_id != automation.identity.id {
        return false;
    }
    match automation.version.plan.self_trigger {
        AutomationSelfTriggerPolicy::SuppressSameVersion => {
            cause.version == automation.version.document.version
        }
        AutomationSelfTriggerPolicy::SuppressSameCorrelation => true,
        AutomationSelfTriggerPolicy::Allow => false,
    }
}

fn trigger_matches(trigger: &ResolvedAutomationTrigger, event: &DomainEvent) -> bool {
    match (trigger, &event.kind) {
        (
            ResolvedAutomationTrigger::ObservationChanged { targets, field },
            DomainEventKind::ObservationChanged {
                endpoint_id,
                capability,
                changed_fields,
            },
        ) => {
            let schema = capability.schema();
            targets.iter().any(|target| {
                exact_target(target, event, endpoint_id, &schema)
                    && field
                        .as_ref()
                        .is_none_or(|field| changed_fields.contains(field))
            })
        }
        (
            ResolvedAutomationTrigger::DeviceEvent {
                targets,
                event: expected,
            },
            DomainEventKind::DeviceEvent {
                endpoint_id,
                event: actual,
            },
        ) => {
            expected == actual
                && targets.iter().any(|target| {
                    event.device_id.as_ref() == Some(&target.device_id)
                        && target.endpoint_id == *endpoint_id
                })
        }
        (
            ResolvedAutomationTrigger::CommandOutcome { targets, states },
            DomainEventKind::CommandTransitioned {
                to,
                endpoint_id,
                capability,
                ..
            },
        ) => {
            states.contains(to)
                && targets.as_ref().is_none_or(|targets| {
                    endpoint_id.as_ref().zip(capability.as_ref()).is_some_and(
                        |(endpoint_id, capability)| {
                            targets
                                .iter()
                                .any(|target| exact_target(target, event, endpoint_id, capability))
                        },
                    )
                })
        }
        _ => false,
    }
}

fn exact_target(
    target: &ResolvedAutomationTarget,
    event: &DomainEvent,
    endpoint_id: &homemagic_domain::EndpointId,
    capability: &str,
) -> bool {
    event.device_id.as_ref() == Some(&target.device_id)
        && target.endpoint_id == *endpoint_id
        && target.capability == capability
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::Utc;
    use homemagic_domain::{
        CapabilityDescriptor, CausationMetadata, CommandId, CommandState, CorrelationId, DeviceId,
        EndpointId, EventId, RiskClass,
    };

    use super::*;

    fn event(kind: DomainEventKind) -> DomainEvent {
        DomainEvent {
            id: EventId::new(),
            device_id: Some(DeviceId::from_native("events", "relay")),
            occurred_at: Utc::now(),
            causation: CausationMetadata {
                correlation_id: CorrelationId::new(),
                causation_event_id: None,
                actor: None,
                automation: None,
            },
            kind,
        }
    }

    fn target() -> ResolvedAutomationTarget {
        ResolvedAutomationTarget {
            device_id: DeviceId::from_native("events", "relay"),
            endpoint_id: EndpointId::new("switch:0"),
            capability: "on_off.v1".to_owned(),
        }
    }

    #[test]
    fn every_normalized_event_trigger_should_match_its_exact_contract() {
        let observation = event(DomainEventKind::ObservationChanged {
            endpoint_id: EndpointId::new("switch:0"),
            capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)
                .unwrap_or_else(|error| panic!("descriptor: {error}")),
            changed_fields: vec!["on".to_owned()],
        });
        assert!(trigger_matches(
            &ResolvedAutomationTrigger::ObservationChanged {
                targets: vec![target()],
                field: Some("on".to_owned()),
            },
            &observation,
        ));
        assert!(!trigger_matches(
            &ResolvedAutomationTrigger::ObservationChanged {
                targets: vec![target()],
                field: Some("brightness".to_owned()),
            },
            &observation,
        ));

        let device = event(DomainEventKind::DeviceEvent {
            endpoint_id: EndpointId::new("switch:0"),
            event: "double_push".to_owned(),
        });
        assert!(trigger_matches(
            &ResolvedAutomationTrigger::DeviceEvent {
                targets: vec![target()],
                event: "double_push".to_owned(),
            },
            &device,
        ));

        let command = event(DomainEventKind::CommandTransitioned {
            command_id: CommandId::new(),
            from: Some(CommandState::Dispatched),
            to: CommandState::Confirmed,
            sequence: 4,
            endpoint_id: Some(EndpointId::new("switch:0")),
            capability: Some("on_off.v1".to_owned()),
        });
        assert!(trigger_matches(
            &ResolvedAutomationTrigger::CommandOutcome {
                targets: Some(vec![target()]),
                states: BTreeSet::from([CommandState::Confirmed]),
            },
            &command,
        ));
        assert!(!trigger_matches(
            &ResolvedAutomationTrigger::CommandOutcome {
                targets: Some(vec![target()]),
                states: BTreeSet::from([CommandState::Failed]),
            },
            &command,
        ));
    }
}

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use homemagic_domain::{
    ActorGrant, ActorId, CapacityState, DeviceId, FreshnessState, GrantScope, PolicyDecision,
    PolicyInput, PolicyReason, RiskClass,
};
use thiserror::Error;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const POLICY_VERSION: u16 = 1;

/// Deterministic default-deny command policy shared by every transport.
#[derive(Clone, Copy, Debug, Default)]
pub struct PolicyEvaluator;

impl PolicyEvaluator {
    /// Evaluates an immutable input and explicit actor grants.
    #[must_use]
    pub fn evaluate(input: &PolicyInput, grants: &[ActorGrant]) -> PolicyDecision {
        let mut reasons = BTreeSet::new();
        if !input.actor.enabled {
            reasons.insert(PolicyReason::ActorDisabled);
        }
        let scoped = grants
            .iter()
            .filter(|grant| grant.actor_id == input.actor.id && grant.enabled)
            .filter(|grant| grant.actions.contains(&input.action))
            .filter(|grant| scope_matches(&grant.scope, input))
            .collect::<Vec<_>>();
        let risk_matched = scoped
            .iter()
            .copied()
            .filter(|grant| grant.maximum_risk.permits(input.risk))
            .collect::<Vec<_>>();
        if scoped.is_empty() {
            reasons.insert(PolicyReason::NoMatchingGrant);
        } else if risk_matched.is_empty() {
            reasons.insert(PolicyReason::RiskExceedsGrant);
        }
        match input.risk {
            RiskClass::Observe | RiskClass::Comfort => {}
            RiskClass::Mechanical => {
                if risk_matched.is_empty() {
                    reasons.insert(PolicyReason::MechanicalGrantRequired);
                }
                if input.freshness != FreshnessState::Fresh {
                    reasons.insert(PolicyReason::StateNotFresh);
                }
                if input.constraint != homemagic_domain::ConstraintState::Available {
                    reasons.insert(PolicyReason::ConstraintUnavailable);
                }
            }
            RiskClass::Security => {
                let exact = risk_matched.iter().any(|grant| {
                    matches!(
                        &grant.scope,
                        GrantScope::Capability {
                            device_id,
                            endpoint_id,
                            schema
                        } if device_id == &input.device_id
                            && endpoint_id == &input.endpoint_id
                            && schema == &input.schema
                    )
                });
                if !exact {
                    reasons.insert(PolicyReason::SecurityExactGrantRequired);
                }
            }
        }
        if input.rate_capacity == CapacityState::Exhausted {
            reasons.insert(PolicyReason::RateLimitExceeded);
        }
        if input.device_capacity == CapacityState::Exhausted {
            reasons.insert(PolicyReason::DeviceConcurrencyExceeded);
        }
        let allowed = reasons.is_empty() && !risk_matched.is_empty();
        if allowed {
            reasons.insert(PolicyReason::AllowedByGrant);
        }
        PolicyDecision {
            policy_version: POLICY_VERSION,
            allowed,
            reasons,
            evaluated_at: input.evaluated_at,
        }
    }
}

fn scope_matches(scope: &GrantScope, input: &PolicyInput) -> bool {
    match scope {
        GrantScope::Installation { installation_id } => {
            installation_id == &input.actor.installation_id
        }
        GrantScope::Space { space_id } => input.spaces.contains(space_id),
        GrantScope::Device { device_id } => device_id == &input.device_id,
        GrantScope::Capability {
            device_id,
            endpoint_id,
            schema,
        } => {
            device_id == &input.device_id
                && endpoint_id == &input.endpoint_id
                && schema == &input.schema
        }
    }
}

/// Bounds for actor request rate and simultaneous work per device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLimitConfig {
    /// Accepted requests per actor within `actor_window`.
    pub actor_requests: usize,
    /// Sliding actor rate window.
    pub actor_window: TimeDelta,
    /// Simultaneous commands allowed for one device.
    pub device_concurrency: usize,
}

impl CommandLimitConfig {
    /// Creates non-zero command safety bounds.
    ///
    /// # Errors
    ///
    /// Rejects empty rates, non-positive windows, or zero device concurrency.
    pub fn new(
        actor_requests: usize,
        actor_window: TimeDelta,
        device_concurrency: usize,
    ) -> Result<Self, CommandLimitConfigError> {
        if actor_requests == 0 || actor_window <= TimeDelta::zero() || device_concurrency == 0 {
            return Err(CommandLimitConfigError);
        }
        Ok(Self {
            actor_requests,
            actor_window,
            device_concurrency,
        })
    }
}

/// Invalid command limit configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("command limits require non-zero rate, positive window, and non-zero concurrency")]
pub struct CommandLimitConfigError;

impl Default for CommandLimitConfig {
    fn default() -> Self {
        Self {
            actor_requests: 60,
            actor_window: TimeDelta::minutes(1),
            device_concurrency: 1,
        }
    }
}

#[derive(Default)]
struct LimitState {
    actor_requests: HashMap<ActorId, VecDeque<DateTime<Utc>>>,
    devices: HashMap<DeviceId, Arc<Semaphore>>,
}

/// Shared bounded command admission state.
#[derive(Clone, Default)]
pub struct CommandLimits {
    config: CommandLimitConfig,
    state: Arc<Mutex<LimitState>>,
}

impl CommandLimits {
    /// Creates limit state with explicit safety bounds.
    #[must_use]
    pub fn new(config: CommandLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(LimitState::default())),
        }
    }

    /// Checks actor rate and atomically tries to acquire one device slot.
    pub async fn try_acquire(
        &self,
        actor_id: &ActorId,
        device_id: &DeviceId,
        now: DateTime<Utc>,
    ) -> CommandLimitCapacities {
        let (rate_capacity, device) =
            {
                let mut state = self.state.lock().await;
                let requests = state.actor_requests.entry(actor_id.clone()).or_default();
                let cutoff = now - self.config.actor_window;
                while requests.front().is_some_and(|at| *at <= cutoff) {
                    requests.pop_front();
                }
                let rate_capacity = if requests.len() < self.config.actor_requests {
                    requests.push_back(now);
                    CapacityState::Available
                } else {
                    CapacityState::Exhausted
                };
                let device =
                    Arc::clone(state.devices.entry(device_id.clone()).or_insert_with(|| {
                        Arc::new(Semaphore::new(self.config.device_concurrency))
                    }));
                (rate_capacity, device)
            };
        let permit = if rate_capacity == CapacityState::Available {
            device
                .try_acquire_owned()
                .ok()
                .map(|permit| CommandPermit { _permit: permit })
        } else {
            None
        };
        let device_capacity = if permit.is_some() {
            CapacityState::Available
        } else {
            CapacityState::Exhausted
        };
        CommandLimitCapacities {
            rate: rate_capacity,
            device: device_capacity,
            permit,
        }
    }
}

/// Admission capacities supplied to deterministic policy evaluation.
pub struct CommandLimitCapacities {
    /// Per-actor rate capacity.
    pub rate: CapacityState,
    /// Per-device concurrency capacity.
    pub device: CapacityState,
    /// Held device slot; dropping it releases capacity.
    pub permit: Option<CommandPermit>,
}

/// RAII ownership of one device command slot.
pub struct CommandPermit {
    _permit: OwnedSemaphorePermit,
}

impl std::fmt::Debug for CommandPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CommandPermit(held)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homemagic_domain::{
        Actor, CommandAction, ConstraintState, EndpointId, GrantId, InstallationId, SpaceId,
    };

    fn input(risk: RiskClass) -> PolicyInput {
        PolicyInput {
            actor: Actor {
                id: ActorId::new(),
                installation_id: InstallationId::new(),
                name: "Agent".to_owned(),
                enabled: true,
                created_at: Utc::now(),
            },
            action: CommandAction::Execute,
            device_id: DeviceId::from_native("test", "device"),
            endpoint_id: EndpointId::new("output:0"),
            schema: "on_off.v1".to_owned(),
            risk,
            spaces: BTreeSet::from([SpaceId::new()]),
            freshness: FreshnessState::Fresh,
            constraint: ConstraintState::Available,
            rate_capacity: CapacityState::Available,
            device_capacity: CapacityState::Available,
            dry_run: false,
            evaluated_at: Utc::now(),
        }
    }

    fn grant(input: &PolicyInput, scope: GrantScope, risk: RiskClass) -> ActorGrant {
        ActorGrant {
            id: GrantId::new(),
            actor_id: input.actor.id.clone(),
            actions: BTreeSet::from([CommandAction::Execute]),
            scope,
            maximum_risk: risk,
            enabled: true,
        }
    }

    #[test]
    fn comfort_should_require_an_explicit_matching_grant() {
        let input = input(RiskClass::Comfort);
        let denied = PolicyEvaluator::evaluate(&input, &[]);
        let allowed = PolicyEvaluator::evaluate(
            &input,
            &[grant(
                &input,
                GrantScope::Device {
                    device_id: input.device_id.clone(),
                },
                RiskClass::Comfort,
            )],
        );

        assert!(!denied.allowed);
        assert!(denied.reasons.contains(&PolicyReason::NoMatchingGrant));
        assert!(allowed.allowed);
    }

    #[test]
    fn mechanical_should_require_risk_freshness_constraints_and_capacity() {
        let mut input = input(RiskClass::Mechanical);
        input.freshness = FreshnessState::Stale;
        input.constraint = ConstraintState::Unavailable;
        input.device_capacity = CapacityState::Exhausted;
        let decision = PolicyEvaluator::evaluate(
            &input,
            &[grant(
                &input,
                GrantScope::Device {
                    device_id: input.device_id.clone(),
                },
                RiskClass::Mechanical,
            )],
        );

        assert!(!decision.allowed);
        assert!(decision.reasons.contains(&PolicyReason::StateNotFresh));
        assert!(
            decision
                .reasons
                .contains(&PolicyReason::ConstraintUnavailable)
        );
        assert!(
            decision
                .reasons
                .contains(&PolicyReason::DeviceConcurrencyExceeded)
        );
    }

    #[test]
    fn security_should_reject_broad_grants_and_accept_exact_capability() {
        let input = input(RiskClass::Security);
        let broad = grant(
            &input,
            GrantScope::Space {
                space_id: input.spaces.iter().next().cloned().unwrap_or_default(),
            },
            RiskClass::Security,
        );
        let exact = grant(
            &input,
            GrantScope::Capability {
                device_id: input.device_id.clone(),
                endpoint_id: input.endpoint_id.clone(),
                schema: input.schema.clone(),
            },
            RiskClass::Security,
        );

        assert!(!PolicyEvaluator::evaluate(&input, &[broad]).allowed);
        assert!(PolicyEvaluator::evaluate(&input, &[exact]).allowed);
    }

    #[test]
    fn administrative_capacity_and_action_denials_should_be_explainable() {
        let mut input = input(RiskClass::Comfort);
        input.actor.enabled = false;
        input.rate_capacity = CapacityState::Exhausted;
        let mut wrong_action = grant(
            &input,
            GrantScope::Device {
                device_id: input.device_id.clone(),
            },
            RiskClass::Comfort,
        );
        wrong_action.actions = BTreeSet::from([CommandAction::ReadAudit]);
        let decision = PolicyEvaluator::evaluate(&input, &[wrong_action]);

        assert!(!decision.allowed);
        assert!(decision.reasons.contains(&PolicyReason::ActorDisabled));
        assert!(decision.reasons.contains(&PolicyReason::RateLimitExceeded));
        assert!(decision.reasons.contains(&PolicyReason::NoMatchingGrant));
    }

    #[test]
    fn space_grants_should_cover_comfort_and_dry_run_should_use_identical_rules() {
        let input = input(RiskClass::Comfort);
        let grant = grant(
            &input,
            GrantScope::Space {
                space_id: input.spaces.iter().next().cloned().unwrap_or_default(),
            },
            RiskClass::Comfort,
        );
        let live = PolicyEvaluator::evaluate(&input, std::slice::from_ref(&grant));
        let mut dry_run = input;
        dry_run.dry_run = true;
        let dry = PolicyEvaluator::evaluate(&dry_run, &[grant]);

        assert!(live.allowed);
        assert_eq!(dry, live);
    }

    #[tokio::test]
    async fn limits_should_bound_actor_rate_and_serialize_each_device() {
        let limits = CommandLimits::new(
            CommandLimitConfig::new(2, TimeDelta::minutes(1), 1)
                .unwrap_or_else(|error| panic!("limits: {error}")),
        );
        let actor = ActorId::new();
        let device = DeviceId::from_native("test", "device");
        let now = Utc::now();
        let first = limits.try_acquire(&actor, &device, now).await;
        let second = limits.try_acquire(&actor, &device, now).await;

        assert_eq!(first.rate, CapacityState::Available);
        assert_eq!(first.device, CapacityState::Available);
        assert_eq!(second.rate, CapacityState::Available);
        assert_eq!(second.device, CapacityState::Exhausted);
        drop(first.permit);
        let third = limits.try_acquire(&actor, &device, now).await;
        assert_eq!(third.rate, CapacityState::Exhausted);
    }
}

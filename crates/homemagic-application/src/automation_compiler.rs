//! Side-effect-free validation and deterministic automation plan compilation.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use chrono::NaiveTime;
use chrono_tz::Tz;
use cron::Schedule;
use homemagic_domain::{
    AutomationAction, AutomationApprovalRequirement, AutomationComparison, AutomationCondition,
    AutomationContentHash, AutomationDeviceReference, AutomationDocument, AutomationExecutionPlan,
    AutomationExpression, AutomationFailurePolicy, AutomationPlanFailurePolicy, AutomationPlanNode,
    AutomationPlanNodeId, AutomationPlanNodeKind, AutomationPlanSchema, AutomationRegistryRevision,
    AutomationResourceBudgetError, AutomationSafetyProfile, AutomationSafetyRequirement,
    AutomationTargetReference, AutomationTrigger, AutomationValidationCode,
    AutomationValidationError, AutomationValue, AutomationValueType, CommandPayload,
    DeviceLifecycle, DeviceRecord, MAX_AUTOMATION_DOCUMENT_BYTES, MAX_AUTOMATION_RETRIES,
    MAX_AUTOMATION_TIMER_MILLIS, PositionCommand, ResolvedAutomationCondition,
    ResolvedAutomationExpression, ResolvedAutomationTarget, ResolvedAutomationTrigger,
    canonical_automation_hash,
};
use serde::Serialize;
use thiserror::Error;

use crate::FoundationSnapshot;

/// All findings produced by one side-effect-free compilation attempt.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("automation validation failed with {} finding(s)", .findings.len())]
pub struct AutomationCompilationError {
    /// Stable, path-addressed findings in discovery order.
    pub findings: Vec<AutomationValidationError>,
}

/// Stateless compiler for immutable authored automation documents.
#[derive(Clone, Copy, Debug, Default)]
pub struct AutomationCompiler;

impl AutomationCompiler {
    /// Validates and compiles an authored document against one registry snapshot.
    ///
    /// This operation is deterministic and performs no I/O or physical action.
    ///
    /// # Errors
    ///
    /// Returns every independently discoverable structural, reference, type, and
    /// resource finding. No partial plan is returned.
    pub fn compile(
        document: &AutomationDocument,
        snapshot: &FoundationSnapshot,
    ) -> Result<AutomationExecutionPlan, AutomationCompilationError> {
        Compiler::new(document, snapshot).compile()
    }
}

struct Compiler<'a> {
    document: &'a AutomationDocument,
    snapshot: &'a FoundationSnapshot,
    findings: Vec<AutomationValidationError>,
    nodes: Vec<AutomationPlanNode>,
    profiles: BTreeSet<AutomationSafetyProfile>,
    requirements: BTreeSet<AutomationSafetyRequirement>,
    next_segment: u32,
}

impl<'a> Compiler<'a> {
    fn new(document: &'a AutomationDocument, snapshot: &'a FoundationSnapshot) -> Self {
        Self {
            document,
            snapshot,
            findings: Vec::new(),
            nodes: Vec::new(),
            profiles: BTreeSet::new(),
            requirements: BTreeSet::new(),
            next_segment: 0,
        }
    }

    fn compile(mut self) -> Result<AutomationExecutionPlan, AutomationCompilationError> {
        self.validate_structure();
        let triggers = self.compile_triggers();
        let condition = self
            .document
            .condition
            .as_ref()
            .and_then(|condition| self.compile_condition(condition, "/condition", 1));

        let complete = self.push_node(AutomationPlanNodeKind::Complete);
        let actions = reduce_actions(&self.document.actions);
        let entry = self.compile_actions(&actions, Some(complete), "/actions", 1);

        if self.nodes.len() > self.document.budget.maximum_nodes as usize {
            self.finding(
                AutomationValidationCode::ResourceBoundExceeded,
                "/budget/maximum_nodes",
                "compiled plan exceeds the declared node budget",
                Some("Increase maximum_nodes or simplify the automation"),
                None,
            );
        }
        if !self.findings.is_empty() {
            return Err(AutomationCompilationError {
                findings: self.findings,
            });
        }

        self.nodes.sort_by_key(|node| node.id);
        let Some(entry) = entry else {
            return Err(AutomationCompilationError {
                findings: vec![AutomationValidationError {
                    code: AutomationValidationCode::RequiredValueMissing,
                    path: "/actions".to_owned(),
                    reason: "automation has no compilable entry action".to_owned(),
                    remediation: Some("Provide at least one valid action".to_owned()),
                    reference: None,
                }],
            });
        };
        let document_hash = match canonical_automation_hash(self.document) {
            Ok(hash) => hash,
            Err(error) => {
                return Err(AutomationCompilationError {
                    findings: vec![AutomationValidationError {
                        code: AutomationValidationCode::RequiredValueMissing,
                        path: String::new(),
                        reason: "automation document cannot be canonicalized".to_owned(),
                        remediation: None,
                        reference: Some(error.to_string()),
                    }],
                });
            }
        };
        let approval = if self.profiles.iter().any(|profile| {
            matches!(
                profile,
                AutomationSafetyProfile::AccessControl
                    | AutomationSafetyProfile::FlowControl
                    | AutomationSafetyProfile::Security
            )
        }) {
            self.requirements
                .insert(AutomationSafetyRequirement::ExplicitApproval);
            AutomationApprovalRequirement::ExplicitUserApproval
        } else {
            AutomationApprovalRequirement::ActivationGrant
        };

        let mut plan = AutomationExecutionPlan {
            schema: AutomationPlanSchema::V1,
            automation_id: self.document.id.clone(),
            automation_version: self.document.version,
            document_hash: document_hash.clone(),
            plan_hash: document_hash,
            registry_revision: AutomationRegistryRevision(self.snapshot.event_cursor.unwrap_or(0)),
            variables: self.document.variables.clone(),
            triggers,
            condition,
            run_mode: self.document.run_mode,
            self_trigger: self.document.self_trigger,
            entry,
            nodes: self.nodes,
            safety_profiles: self.profiles,
            safety_requirements: self.requirements,
            approval,
            budget: self.document.budget,
        };
        plan.plan_hash = match hash_plan_without_hash(&plan) {
            Ok(hash) => hash,
            Err(error) => {
                return Err(AutomationCompilationError {
                    findings: vec![AutomationValidationError {
                        code: AutomationValidationCode::RequiredValueMissing,
                        path: String::new(),
                        reason: "normalized plan cannot be canonicalized".to_owned(),
                        remediation: None,
                        reference: Some(error.to_string()),
                    }],
                });
            }
        };
        Ok(plan)
    }

    fn validate_structure(&mut self) {
        match serde_json::to_vec(self.document) {
            Ok(bytes) if bytes.len() > MAX_AUTOMATION_DOCUMENT_BYTES => self.finding(
                AutomationValidationCode::DocumentTooLarge,
                "",
                "serialized automation document exceeds the hard byte limit",
                Some("Reduce document size"),
                None,
            ),
            Err(error) => self.finding(
                AutomationValidationCode::RequiredValueMissing,
                "",
                "automation document cannot be serialized",
                None,
                Some(error.to_string()),
            ),
            Ok(_) => {}
        }
        if self.document.name.trim().is_empty() {
            self.required("/name", "automation name must not be empty");
        }
        if self.document.provenance.source_request.trim().is_empty() {
            self.required(
                "/provenance/source_request",
                "source request must not be empty",
            );
        }
        if self.document.provenance.rationale.trim().is_empty() {
            self.required("/provenance/rationale", "rationale must not be empty");
        }
        if self.document.triggers.is_empty() {
            self.required("/triggers", "at least one trigger is required");
        }
        if self.document.actions.is_empty() {
            self.required("/actions", "at least one action is required");
        }
        if let Err(error) = self.document.budget.validate() {
            let field = budget_field(error);
            self.finding(
                AutomationValidationCode::ResourceBoundExceeded,
                &format!("/budget/{field}"),
                "declared budget is zero or exceeds the engine limit",
                Some("Choose a positive value within the documented engine bound"),
                None,
            );
        }
        match self.document.run_mode {
            homemagic_domain::AutomationRunMode::Queued { capacity }
                if capacity == 0 || capacity > self.document.budget.maximum_queue_length =>
            {
                self.bound(
                    "/run_mode/capacity",
                    "queue capacity exceeds its declared budget",
                );
            }
            homemagic_domain::AutomationRunMode::Parallel { maximum_parallel }
                if maximum_parallel == 0
                    || maximum_parallel > self.document.budget.maximum_parallel_width =>
            {
                self.bound(
                    "/run_mode/maximum_parallel",
                    "parallel run capacity exceeds its declared budget",
                );
            }
            _ => {}
        }
        for (name, definition) in &self.document.variables {
            let path = format!("/variables/{}", pointer_segment(name));
            if name.trim().is_empty() {
                self.required(&path, "variable name must not be empty");
            }
            if let Some(initial) = &definition.initial {
                self.validate_value(initial, &format!("{path}/initial"));
                if initial.value_type() != definition.value_type {
                    self.type_mismatch(
                        &format!("{path}/initial"),
                        "initial value does not match the declared variable type",
                    );
                }
            }
        }
    }

    fn compile_triggers(&mut self) -> Vec<ResolvedAutomationTrigger> {
        let mut compiled = Vec::new();
        for (index, trigger) in self.document.triggers.iter().enumerate() {
            let path = format!("/triggers/{index}");
            let resolved = match trigger {
                AutomationTrigger::ObservationChanged { target, field } => {
                    let targets = self.resolve_target(target, &format!("{path}/target"));
                    if let Some(field) = field {
                        self.validate_observation_field(
                            &target.capability,
                            field,
                            &format!("{path}/field"),
                        );
                    }
                    targets.map(|targets| ResolvedAutomationTrigger::ObservationChanged {
                        targets,
                        field: field.clone(),
                    })
                }
                AutomationTrigger::DeviceEvent { target, event } => {
                    if event.trim().is_empty() {
                        self.required(&format!("{path}/event"), "event name must not be empty");
                    }
                    self.resolve_target(target, &format!("{path}/target"))
                        .map(|targets| ResolvedAutomationTrigger::DeviceEvent {
                            targets,
                            event: event.clone(),
                        })
                }
                AutomationTrigger::Schedule { schedule } => {
                    self.validate_schedule(schedule, &format!("{path}/schedule"));
                    Some(ResolvedAutomationTrigger::Schedule {
                        schedule: schedule.clone(),
                    })
                }
                AutomationTrigger::CommandOutcome { target, states } => {
                    if states.is_empty() {
                        self.required(&format!("{path}/states"), "at least one state is required");
                    }
                    let targets = target
                        .as_ref()
                        .and_then(|target| self.resolve_target(target, &format!("{path}/target")));
                    if target.is_some() && targets.is_none() {
                        None
                    } else {
                        Some(ResolvedAutomationTrigger::CommandOutcome {
                            targets,
                            states: states.clone(),
                        })
                    }
                }
            };
            if let Some(resolved) = resolved {
                compiled.push(resolved);
            }
        }
        compiled
    }

    fn compile_actions(
        &mut self,
        actions: &[AutomationAction],
        next: Option<AutomationPlanNodeId>,
        path: &str,
        depth: u16,
    ) -> Option<AutomationPlanNodeId> {
        if depth > self.document.budget.maximum_nesting_depth {
            self.bound(path, "action nesting exceeds the declared depth budget");
            return next;
        }
        let mut active_segment = None;
        let segments: Vec<_> = actions
            .iter()
            .map(|action| {
                if matches!(action, AutomationAction::Command { .. }) {
                    Some(*active_segment.get_or_insert_with(|| {
                        let segment = self.next_segment;
                        self.next_segment = self.next_segment.saturating_add(1);
                        segment
                    }))
                } else {
                    active_segment = None;
                    None
                }
            })
            .collect();
        let mut following = next;
        for (index, action) in actions.iter().enumerate().rev() {
            following = self.compile_action(
                action,
                following,
                &format!("{path}/{index}"),
                depth,
                segments[index],
            );
        }
        following
    }

    #[allow(clippy::too_many_lines)]
    fn compile_action(
        &mut self,
        action: &AutomationAction,
        next: Option<AutomationPlanNodeId>,
        path: &str,
        depth: u16,
        reduction_segment: Option<u32>,
    ) -> Option<AutomationPlanNodeId> {
        match action {
            AutomationAction::Command {
                target,
                payload,
                retry,
                on_failure,
            } => {
                if payload.schema() != target.capability {
                    self.incompatible(
                        &format!("{path}/payload"),
                        "command payload schema does not match the target capability",
                    );
                }
                if let Err(error) = payload.validate() {
                    self.incompatible(
                        &format!("{path}/payload"),
                        &format!("command payload is invalid: {error:?}"),
                    );
                }
                if retry.maximum_retries > MAX_AUTOMATION_RETRIES {
                    self.bound(
                        &format!("{path}/retry/maximum_retries"),
                        "retry count exceeds the engine limit",
                    );
                }
                if retry.backoff_ms > MAX_AUTOMATION_TIMER_MILLIS {
                    self.bound(
                        &format!("{path}/retry/backoff_ms"),
                        "retry delay exceeds the engine timer limit",
                    );
                }
                let targets = self.resolve_target(target, &format!("{path}/target"));
                let failure = self.compile_failure(
                    on_failure,
                    next,
                    &format!("{path}/on_failure"),
                    depth + 1,
                );
                targets.map(|targets| {
                    self.classify_command(target, payload, path);
                    self.push_node(AutomationPlanNodeKind::Command {
                        targets,
                        payload: payload.clone(),
                        reduction_segment: reduction_segment.unwrap_or_default(),
                        retry: retry.clone(),
                        on_failure: failure,
                        next,
                    })
                })
            }
            AutomationAction::Delay { duration_ms } => {
                self.validate_timer(*duration_ms, &format!("{path}/duration_ms"));
                Some(self.push_node(AutomationPlanNodeKind::Delay {
                    duration_ms: *duration_ms,
                    next,
                }))
            }
            AutomationAction::Wait {
                condition,
                timeout_ms,
                on_timeout,
            } => {
                self.validate_timer(*timeout_ms, &format!("{path}/timeout_ms"));
                let condition =
                    self.compile_condition(condition, &format!("{path}/condition"), depth + 1);
                let on_timeout = self.compile_failure(
                    on_timeout,
                    next,
                    &format!("{path}/on_timeout"),
                    depth + 1,
                );
                condition.map(|condition| {
                    self.push_node(AutomationPlanNodeKind::Wait {
                        condition,
                        timeout_ms: *timeout_ms,
                        on_timeout,
                        next,
                    })
                })
            }
            AutomationAction::SetVariable { name, value } => {
                let expression = self.compile_expression(value, &format!("{path}/value"));
                match (self.document.variables.get(name), expression) {
                    (None, _) => {
                        self.type_mismatch(
                            &format!("{path}/name"),
                            "assigned variable is not declared",
                        );
                        None
                    }
                    (Some(definition), Some((value, value_type))) => {
                        if definition.value_type != value_type {
                            self.type_mismatch(
                                &format!("{path}/value"),
                                "assigned expression does not match the variable type",
                            );
                        }
                        Some(self.push_node(AutomationPlanNodeKind::SetVariable {
                            name: name.clone(),
                            value,
                            next,
                        }))
                    }
                    (Some(_), None) => None,
                }
            }
            AutomationAction::Sequence { actions } => self.compile_actions(
                &reduce_actions(actions),
                next,
                &format!("{path}/actions"),
                depth + 1,
            ),
            AutomationAction::If {
                condition,
                then_actions,
                else_actions,
            } => {
                if matches!(condition, AutomationCondition::Literal { .. }) {
                    self.finding(
                        AutomationValidationCode::ImpossibleBranch,
                        &format!("{path}/condition"),
                        "literal condition makes one branch unreachable",
                        Some("Remove the unreachable branch or use a dynamic condition"),
                        None,
                    );
                }
                let join = self.push_node(AutomationPlanNodeKind::Join { next });
                let then_node = self.compile_actions(
                    &reduce_actions(then_actions),
                    Some(join),
                    &format!("{path}/then_actions"),
                    depth + 1,
                );
                let else_node = self.compile_actions(
                    &reduce_actions(else_actions),
                    Some(join),
                    &format!("{path}/else_actions"),
                    depth + 1,
                );
                self.compile_condition(condition, &format!("{path}/condition"), depth + 1)
                    .map(|condition| {
                        self.push_node(AutomationPlanNodeKind::Branch {
                            condition,
                            then_node,
                            else_node,
                            join: Some(join),
                        })
                    })
            }
            AutomationAction::Parallel {
                branches,
                maximum_parallel,
            } => Some(self.compile_group(branches, *maximum_parallel, next, path, depth, false)),
            AutomationAction::Race {
                branches,
                maximum_parallel,
            } => Some(self.compile_group(branches, *maximum_parallel, next, path, depth, true)),
        }
    }

    fn compile_group(
        &mut self,
        branches: &[Vec<AutomationAction>],
        maximum_parallel: u16,
        next: Option<AutomationPlanNodeId>,
        path: &str,
        depth: u16,
        race: bool,
    ) -> AutomationPlanNodeId {
        if branches.is_empty() {
            self.required(
                &format!("{path}/branches"),
                "at least one branch is required",
            );
        }
        if branches.len() > self.document.budget.maximum_parallel_width as usize
            || maximum_parallel == 0
            || usize::from(maximum_parallel) > branches.len()
            || maximum_parallel > self.document.budget.maximum_parallel_width
        {
            self.bound(
                &format!("{path}/maximum_parallel"),
                "parallel width exceeds the branch count or declared budget",
            );
        }
        let join = self.push_node(AutomationPlanNodeKind::Join { next });
        let mut entries = Vec::new();
        for (index, branch) in branches.iter().enumerate() {
            if branch.is_empty() {
                self.required(
                    &format!("{path}/branches/{index}"),
                    "parallel branch must not be empty",
                );
            }
            if let Some(entry) = self.compile_actions(
                &reduce_actions(branch),
                Some(join),
                &format!("{path}/branches/{index}"),
                depth + 1,
            ) {
                entries.push(entry);
            }
        }
        let kind = if race {
            AutomationPlanNodeKind::Race {
                branches: entries,
                maximum_parallel,
                join: Some(join),
            }
        } else {
            AutomationPlanNodeKind::Parallel {
                branches: entries,
                maximum_parallel,
                join: Some(join),
            }
        };
        self.push_node(kind)
    }

    fn compile_failure(
        &mut self,
        policy: &AutomationFailurePolicy,
        next: Option<AutomationPlanNodeId>,
        path: &str,
        depth: u16,
    ) -> AutomationPlanFailurePolicy {
        match policy {
            AutomationFailurePolicy::StopRun => AutomationPlanFailurePolicy::StopRun,
            AutomationFailurePolicy::StopBranch => AutomationPlanFailurePolicy::StopBranch,
            AutomationFailurePolicy::Continue => AutomationPlanFailurePolicy::Continue,
            AutomationFailurePolicy::Fallback { actions } => {
                if actions.is_empty() {
                    self.required(
                        &format!("{path}/actions"),
                        "fallback actions must not be empty",
                    );
                }
                let entry = self.compile_actions(
                    &reduce_actions(actions),
                    next,
                    &format!("{path}/actions"),
                    depth,
                );
                AutomationPlanFailurePolicy::Fallback { entry }
            }
        }
    }

    fn compile_condition(
        &mut self,
        condition: &AutomationCondition,
        path: &str,
        depth: u16,
    ) -> Option<ResolvedAutomationCondition> {
        if depth > self.document.budget.maximum_nesting_depth {
            self.bound(path, "condition nesting exceeds the declared depth budget");
            return None;
        }
        match condition {
            AutomationCondition::Literal { value } => {
                Some(ResolvedAutomationCondition::Literal { value: *value })
            }
            AutomationCondition::Compare {
                left,
                operator,
                right,
            } => {
                let left = self.compile_expression(left, &format!("{path}/left"));
                let right = self.compile_expression(right, &format!("{path}/right"));
                match (left, right) {
                    (Some((left, left_type)), Some((right, right_type))) => {
                        if left_type != right_type {
                            self.type_mismatch(path, "comparison operand types do not match");
                        }
                        if !matches!(
                            operator,
                            AutomationComparison::Equal | AutomationComparison::NotEqual
                        ) && matches!(
                            left_type,
                            AutomationValueType::Null | AutomationValueType::Boolean
                        ) {
                            self.type_mismatch(
                                path,
                                "selected operator is not defined for this value type",
                            );
                        }
                        Some(ResolvedAutomationCondition::Compare {
                            left,
                            operator: *operator,
                            right,
                        })
                    }
                    _ => None,
                }
            }
            AutomationCondition::All { conditions } => {
                self.compile_condition_list(conditions, path, depth, |conditions| {
                    ResolvedAutomationCondition::All { conditions }
                })
            }
            AutomationCondition::Any { conditions } => {
                self.compile_condition_list(conditions, path, depth, |conditions| {
                    ResolvedAutomationCondition::Any { conditions }
                })
            }
            AutomationCondition::Not { condition } => self
                .compile_condition(condition, &format!("{path}/condition"), depth + 1)
                .map(|condition| ResolvedAutomationCondition::Not {
                    condition: Box::new(condition),
                }),
            AutomationCondition::TimeWindow {
                timezone,
                start,
                end,
            } => {
                if Tz::from_str(timezone).is_err()
                    || NaiveTime::parse_from_str(start, "%H:%M:%S").is_err()
                    || NaiveTime::parse_from_str(end, "%H:%M:%S").is_err()
                {
                    self.invalid_schedule(path, "time window or timezone is invalid");
                }
                Some(ResolvedAutomationCondition::TimeWindow {
                    timezone: timezone.clone(),
                    start: start.clone(),
                    end: end.clone(),
                })
            }
            AutomationCondition::StateDuration {
                condition,
                duration_ms,
            } => {
                self.validate_timer(*duration_ms, &format!("{path}/duration_ms"));
                self.compile_condition(condition, &format!("{path}/condition"), depth + 1)
                    .map(|condition| ResolvedAutomationCondition::StateDuration {
                        condition: Box::new(condition),
                        duration_ms: *duration_ms,
                    })
            }
        }
    }

    fn compile_condition_list(
        &mut self,
        conditions: &[AutomationCondition],
        path: &str,
        depth: u16,
        constructor: impl FnOnce(Vec<ResolvedAutomationCondition>) -> ResolvedAutomationCondition,
    ) -> Option<ResolvedAutomationCondition> {
        if conditions.is_empty() {
            self.required(
                &format!("{path}/conditions"),
                "condition list must not be empty",
            );
        }
        let compiled: Vec<_> = conditions
            .iter()
            .enumerate()
            .filter_map(|(index, condition)| {
                self.compile_condition(condition, &format!("{path}/conditions/{index}"), depth + 1)
            })
            .collect();
        (compiled.len() == conditions.len()).then(|| constructor(compiled))
    }

    fn compile_expression(
        &mut self,
        expression: &AutomationExpression,
        path: &str,
    ) -> Option<(ResolvedAutomationExpression, AutomationValueType)> {
        match expression {
            AutomationExpression::Literal { value } => {
                self.validate_value(value, &format!("{path}/value"));
                Some((
                    ResolvedAutomationExpression::Literal {
                        value: value.clone(),
                    },
                    value.value_type(),
                ))
            }
            AutomationExpression::Variable { name } => {
                let Some(definition) = self.document.variables.get(name) else {
                    self.type_mismatch(path, "referenced variable is not declared");
                    return None;
                };
                Some((
                    ResolvedAutomationExpression::Variable { name: name.clone() },
                    definition.value_type,
                ))
            }
            AutomationExpression::Observation { target, field } => {
                let value_type = self.validate_observation_field(
                    &target.capability,
                    field,
                    &format!("{path}/field"),
                );
                let targets = self.resolve_target(target, &format!("{path}/target"));
                match (targets, value_type) {
                    (Some(targets), Some(value_type)) => Some((
                        ResolvedAutomationExpression::Observation {
                            targets,
                            field: field.clone(),
                        },
                        value_type,
                    )),
                    _ => None,
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_target(
        &mut self,
        target: &AutomationTargetReference,
        path: &str,
    ) -> Option<Vec<ResolvedAutomationTarget>> {
        let devices: Vec<&DeviceRecord> = match &target.device {
            AutomationDeviceReference::Device { device_id } => self
                .snapshot
                .devices
                .iter()
                .filter(|device| &device.snapshot.id == device_id)
                .collect(),
            AutomationDeviceReference::Alias { alias } => self
                .snapshot
                .devices
                .iter()
                .filter(|device| device.snapshot.name == *alias || device.aliases.contains(alias))
                .collect(),
            AutomationDeviceReference::Space { space_id } => self
                .snapshot
                .devices
                .iter()
                .filter(|device| device.spaces.contains(space_id))
                .collect(),
        };
        if devices.is_empty() {
            self.finding(
                AutomationValidationCode::ReferenceMissing,
                &format!("{path}/device"),
                "device selector did not resolve",
                Some("Use an enrolled device ID, unique alias, or populated space"),
                None,
            );
            return None;
        }
        if matches!(target.device, AutomationDeviceReference::Alias { .. }) && devices.len() > 1 {
            self.finding(
                AutomationValidationCode::ReferenceAmbiguous,
                &format!("{path}/device"),
                "device alias resolves to more than one device",
                Some("Use a unique alias or exact device ID"),
                None,
            );
            return None;
        }
        if devices
            .iter()
            .any(|device| device.lifecycle != DeviceLifecycle::Enrolled)
        {
            self.finding(
                AutomationValidationCode::ReferenceStale,
                &format!("{path}/device"),
                "device selector includes a stale or removed device",
                Some("Restore device health or select an enrolled device"),
                None,
            );
            return None;
        }

        let mut resolved = Vec::new();
        for device in devices {
            let endpoints: Vec<_> = device
                .capability_descriptors
                .iter()
                .filter(|(endpoint, descriptors)| {
                    target
                        .endpoint_id
                        .as_ref()
                        .is_none_or(|selected| selected == *endpoint)
                        && descriptors
                            .iter()
                            .any(|descriptor| descriptor.schema() == target.capability)
                })
                .map(|(endpoint, _)| endpoint.clone())
                .collect();
            if endpoints.is_empty() {
                self.incompatible(
                    path,
                    "target endpoint does not expose the requested capability schema",
                );
                return None;
            }
            if target.endpoint_id.is_none() && endpoints.len() > 1 {
                self.finding(
                    AutomationValidationCode::ReferenceAmbiguous,
                    &format!("{path}/endpoint_id"),
                    "capability exists on multiple endpoints",
                    Some("Specify an exact endpoint_id"),
                    Some(device.snapshot.id.to_string()),
                );
                return None;
            }
            resolved.extend(
                endpoints
                    .into_iter()
                    .map(|endpoint_id| ResolvedAutomationTarget {
                        device_id: device.snapshot.id.clone(),
                        endpoint_id,
                        capability: target.capability.clone(),
                    }),
            );
        }
        resolved.sort_by(|left, right| {
            (&left.device_id, &left.endpoint_id).cmp(&(&right.device_id, &right.endpoint_id))
        });
        Some(resolved)
    }

    fn validate_observation_field(
        &mut self,
        capability: &str,
        field: &str,
        path: &str,
    ) -> Option<AutomationValueType> {
        let value_type = match (capability, field) {
            ("on_off.v1", "on") | ("availability.v1", "online") => AutomationValueType::Boolean,
            ("level.v1" | "position.v1", "percent")
            | ("power.v1", "watts" | "volts" | "amperes")
            | ("energy.v1", "watt_hours") => AutomationValueType::Decimal,
            ("position.v1", "motion") | ("diagnostics.v1", "firmware_version") => {
                AutomationValueType::String
            }
            _ => {
                self.incompatible(path, "field is not defined by the capability schema");
                return None;
            }
        };
        Some(value_type)
    }

    fn validate_schedule(&mut self, schedule: &homemagic_domain::AutomationSchedule, path: &str) {
        if schedule.cron.split_whitespace().count() != 5
            || Schedule::from_str(&format!("0 {}", schedule.cron)).is_err()
            || Tz::from_str(&schedule.timezone).is_err()
        {
            self.invalid_schedule(
                path,
                "five-field cron expression or IANA timezone is invalid",
            );
        }
        if schedule.occurrence_window_ms == 0
            || schedule.occurrence_window_ms > MAX_AUTOMATION_TIMER_MILLIS
        {
            self.bound(
                &format!("{path}/occurrence_window_ms"),
                "occurrence window is outside the supported timer range",
            );
        }
    }

    fn classify_command(
        &mut self,
        target: &AutomationTargetReference,
        payload: &CommandPayload,
        path: &str,
    ) {
        let name = target.capability.to_ascii_lowercase();
        let profile = if name.contains("lock") || name.contains("door") {
            AutomationSafetyProfile::AccessControl
        } else if name.contains("valve") || name.contains("flow") {
            AutomationSafetyProfile::FlowControl
        } else if name.contains("camera") || name.contains("security") {
            AutomationSafetyProfile::Security
        } else if matches!(payload, CommandPayload::Position(_)) {
            self.requirements
                .insert(AutomationSafetyRequirement::FreshState);
            self.requirements
                .insert(AutomationSafetyRequirement::StopSupport);
            if matches!(
                payload,
                CommandPayload::Position(PositionCommand::GoTo { .. })
            ) {
                self.requirements
                    .insert(AutomationSafetyRequirement::Calibration);
                self.requirements
                    .insert(AutomationSafetyRequirement::Position);
                if !self.position_is_calibrated(target) {
                    self.finding(
                        AutomationValidationCode::SafetyConstraintUnavailable,
                        &format!("{path}/payload"),
                        "absolute position command requires a calibrated current position",
                        Some("Calibrate the cover and refresh its position before validation"),
                        None,
                    );
                }
            }
            AutomationSafetyProfile::ComfortMotion
        } else {
            AutomationSafetyProfile::Comfort
        };
        self.profiles.insert(profile);
    }

    fn position_is_calibrated(&self, target: &AutomationTargetReference) -> bool {
        let Some(endpoint_id) = &target.endpoint_id else {
            return false;
        };
        self.snapshot.devices.iter().any(|device| {
            let selected = match &target.device {
                AutomationDeviceReference::Device { device_id } => &device.snapshot.id == device_id,
                AutomationDeviceReference::Alias { alias } => {
                    device.snapshot.name == *alias || device.aliases.contains(alias)
                }
                AutomationDeviceReference::Space { space_id } => device.spaces.contains(space_id),
            };
            selected
                && device.snapshot.endpoints.iter().any(|endpoint| {
                    &endpoint.id == endpoint_id
                        && endpoint.capabilities.iter().any(|capability| {
                            matches!(
                                capability,
                                homemagic_domain::CapabilitySnapshot::Position {
                                    percent: Some(_),
                                    ..
                                }
                            )
                        })
                })
        })
    }

    fn validate_value(&mut self, value: &AutomationValue, path: &str) {
        if let AutomationValue::Decimal(decimal) = value {
            if !canonical_decimal(decimal) {
                self.finding(
                    AutomationValidationCode::InvalidDecimal,
                    path,
                    "decimal is not canonical plain base-10 text",
                    Some("Use forms such as 0, -2, or 12.5 without exponent or trailing zeros"),
                    None,
                );
            }
        }
        if let AutomationValue::DurationMillis(duration) = value {
            self.validate_timer(*duration, path);
        }
    }

    fn validate_timer(&mut self, duration_ms: u64, path: &str) {
        if duration_ms == 0 || duration_ms > MAX_AUTOMATION_TIMER_MILLIS {
            self.bound(path, "duration is outside the supported timer range");
        }
    }

    fn push_node(&mut self, kind: AutomationPlanNodeKind) -> AutomationPlanNodeId {
        let id = AutomationPlanNodeId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));
        self.nodes.push(AutomationPlanNode {
            id,
            order: id.0,
            kind,
        });
        id
    }

    fn required(&mut self, path: &str, reason: &str) {
        self.finding(
            AutomationValidationCode::RequiredValueMissing,
            path,
            reason,
            None,
            None,
        );
    }

    fn bound(&mut self, path: &str, reason: &str) {
        self.finding(
            AutomationValidationCode::ResourceBoundExceeded,
            path,
            reason,
            None,
            None,
        );
    }

    fn incompatible(&mut self, path: &str, reason: &str) {
        self.finding(
            AutomationValidationCode::CapabilityIncompatible,
            path,
            reason,
            Some("Choose a compatible capability, endpoint, field, or payload"),
            None,
        );
    }

    fn type_mismatch(&mut self, path: &str, reason: &str) {
        self.finding(
            AutomationValidationCode::TypeMismatch,
            path,
            reason,
            Some("Use matching declared scalar types"),
            None,
        );
    }

    fn invalid_schedule(&mut self, path: &str, reason: &str) {
        self.finding(
            AutomationValidationCode::InvalidSchedule,
            path,
            reason,
            Some("Use five-field cron syntax and an IANA timezone name"),
            None,
        );
    }

    fn finding(
        &mut self,
        code: AutomationValidationCode,
        path: &str,
        reason: &str,
        remediation: Option<&str>,
        reference: Option<String>,
    ) {
        self.findings.push(AutomationValidationError {
            code,
            path: path.to_owned(),
            reason: reason.to_owned(),
            remediation: remediation.map(str::to_owned),
            reference,
        });
    }
}

fn budget_field(error: AutomationResourceBudgetError) -> &'static str {
    match error {
        AutomationResourceBudgetError::MaximumNodes => "maximum_nodes",
        AutomationResourceBudgetError::MaximumNestingDepth => "maximum_nesting_depth",
        AutomationResourceBudgetError::MaximumParallelWidth => "maximum_parallel_width",
        AutomationResourceBudgetError::MaximumQueueLength => "maximum_queue_length",
        AutomationResourceBudgetError::MaximumTraceSteps => "maximum_trace_steps",
        AutomationResourceBudgetError::MaximumRunDuration => "maximum_run_duration_ms",
    }
}

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn canonical_decimal(value: &str) -> bool {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    if unsigned.is_empty() || unsigned.starts_with('+') {
        return false;
    }
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        return false;
    }
    match fraction {
        Some(fraction) => {
            !fraction.is_empty()
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
                && !fraction.ends_with('0')
        }
        None => value != "-0",
    }
}

fn reduce_actions(actions: &[AutomationAction]) -> Vec<AutomationAction> {
    let mut output = Vec::new();
    let mut segment = Vec::new();
    let flush = |segment: &mut Vec<AutomationAction>, output: &mut Vec<AutomationAction>| {
        let commands = std::mem::take(segment);
        for (index, action) in commands.iter().enumerate() {
            let AutomationAction::Command { target, .. } = action else {
                unreachable!("segments contain commands only");
            };
            let is_last = !commands.iter().skip(index + 1).any(|candidate| {
                matches!(candidate, AutomationAction::Command { target: later, .. } if later == target)
            });
            if is_last {
                output.push(action.clone());
            }
        }
    };
    for action in actions {
        if matches!(action, AutomationAction::Command { .. }) {
            segment.push(action.clone());
        } else {
            flush(&mut segment, &mut output);
            output.push(reduce_nested(action));
        }
    }
    flush(&mut segment, &mut output);
    output
}

fn reduce_nested(action: &AutomationAction) -> AutomationAction {
    match action {
        AutomationAction::Sequence { actions } => AutomationAction::Sequence {
            actions: reduce_actions(actions),
        },
        AutomationAction::If {
            condition,
            then_actions,
            else_actions,
        } => AutomationAction::If {
            condition: condition.clone(),
            then_actions: reduce_actions(then_actions),
            else_actions: reduce_actions(else_actions),
        },
        AutomationAction::Parallel {
            branches,
            maximum_parallel,
        } => AutomationAction::Parallel {
            branches: branches
                .iter()
                .map(|branch| reduce_actions(branch))
                .collect(),
            maximum_parallel: *maximum_parallel,
        },
        AutomationAction::Race {
            branches,
            maximum_parallel,
        } => AutomationAction::Race {
            branches: branches
                .iter()
                .map(|branch| reduce_actions(branch))
                .collect(),
            maximum_parallel: *maximum_parallel,
        },
        AutomationAction::Wait {
            condition,
            timeout_ms,
            on_timeout,
        } => AutomationAction::Wait {
            condition: condition.clone(),
            timeout_ms: *timeout_ms,
            on_timeout: reduce_failure(on_timeout),
        },
        AutomationAction::Command { .. }
        | AutomationAction::Delay { .. }
        | AutomationAction::SetVariable { .. } => action.clone(),
    }
}

fn reduce_failure(policy: &AutomationFailurePolicy) -> AutomationFailurePolicy {
    match policy {
        AutomationFailurePolicy::Fallback { actions } => AutomationFailurePolicy::Fallback {
            actions: reduce_actions(actions),
        },
        _ => policy.clone(),
    }
}

fn hash_plan_without_hash(
    plan: &AutomationExecutionPlan,
) -> Result<AutomationContentHash, homemagic_domain::CanonicalAutomationError> {
    #[derive(Serialize)]
    struct HashablePlan<'a> {
        schema: &'a AutomationPlanSchema,
        automation_id: &'a homemagic_domain::AutomationId,
        automation_version: homemagic_domain::AutomationVersion,
        document_hash: &'a AutomationContentHash,
        registry_revision: AutomationRegistryRevision,
        variables: &'a BTreeMap<String, homemagic_domain::AutomationVariableDefinition>,
        triggers: &'a [ResolvedAutomationTrigger],
        condition: &'a Option<ResolvedAutomationCondition>,
        run_mode: homemagic_domain::AutomationRunMode,
        self_trigger: homemagic_domain::AutomationSelfTriggerPolicy,
        entry: AutomationPlanNodeId,
        nodes: &'a [AutomationPlanNode],
        safety_profiles: &'a BTreeSet<AutomationSafetyProfile>,
        safety_requirements: &'a BTreeSet<AutomationSafetyRequirement>,
        approval: AutomationApprovalRequirement,
        budget: homemagic_domain::AutomationResourceBudget,
    }
    canonical_automation_hash(&HashablePlan {
        schema: &plan.schema,
        automation_id: &plan.automation_id,
        automation_version: plan.automation_version,
        document_hash: &plan.document_hash,
        registry_revision: plan.registry_revision,
        variables: &plan.variables,
        triggers: &plan.triggers,
        condition: &plan.condition,
        run_mode: plan.run_mode,
        self_trigger: plan.self_trigger,
        entry: plan.entry,
        nodes: &plan.nodes,
        safety_profiles: &plan.safety_profiles,
        safety_requirements: &plan.safety_requirements,
        approval: plan.approval,
        budget: plan.budget,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use chrono::{TimeZone, Utc};
    use homemagic_domain::{
        ActorId, AutomationDocumentSchema, AutomationFailurePolicy, AutomationId,
        AutomationProvenance, AutomationResourceBudget, AutomationRetryPolicy, AutomationRunMode,
        AutomationSchedule, AutomationSelfTriggerPolicy, AutomationVariableDefinition,
        AutomationVersion, CapabilitySnapshot, CommandErrorCode, DeviceId, DeviceSnapshot,
        EndpointId, EndpointSnapshot, InstallationId, IntegrationId, LifecycleTrigger,
        OnOffCommand, RiskClass,
    };

    #[test]
    fn canonical_decimal_contract_is_strict() {
        for valid in ["0", "1", "-1", "12.5", "0.01"] {
            assert!(canonical_decimal(valid), "{valid}");
        }
        for invalid in ["", "+1", "01", "1.0", "1e2", "-0", ".5", "1."] {
            assert!(!canonical_decimal(invalid), "{invalid}");
        }
    }

    #[test]
    fn compilation_is_deterministic_and_reduces_to_last_desired_state() {
        let (snapshot, target) = fixture();
        let mut document = document(target.clone());
        document.actions = vec![
            command(target.clone(), false),
            command(target.clone(), true),
            command(target, false),
        ];

        let first = AutomationCompiler::compile(&document, &snapshot).expect("valid plan");
        let second = AutomationCompiler::compile(&document, &snapshot).expect("valid plan");

        assert_eq!(first, second);
        assert_eq!(first.document_hash, second.document_hash);
        assert_eq!(first.plan_hash, second.plan_hash);
        let commands: Vec<_> = first
            .nodes
            .iter()
            .filter_map(|node| match &node.kind {
                AutomationPlanNodeKind::Command {
                    payload, targets, ..
                } => Some((payload, targets)),
                _ => None,
            })
            .collect();
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].0,
            &CommandPayload::OnOff(OnOffCommand::Set { on: false })
        );
        assert_eq!(commands[0].1.len(), 1);
    }

    #[test]
    fn delay_is_a_reduction_boundary() {
        let (snapshot, target) = fixture();
        let mut document = document(target.clone());
        document.actions = vec![
            command(target.clone(), true),
            command(target.clone(), false),
            AutomationAction::Delay { duration_ms: 10 },
            command(target, true),
        ];

        let plan = AutomationCompiler::compile(&document, &snapshot).expect("valid plan");
        let commands = plan
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, AutomationPlanNodeKind::Command { .. }))
            .count();
        assert_eq!(commands, 2);
    }

    #[test]
    fn missing_reference_reports_exact_json_pointer() {
        let (snapshot, mut target) = fixture();
        target.device = AutomationDeviceReference::Device {
            device_id: DeviceId::from_native("missing", "device"),
        };
        let error = AutomationCompiler::compile(&document(target), &snapshot)
            .expect_err("missing target must fail");

        assert!(error.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::ReferenceMissing
                && finding.path == "/actions/0/target/device"
        }));
    }

    #[test]
    fn ambiguous_alias_reports_exact_json_pointer() {
        let (mut snapshot, mut target) = fixture();
        let mut duplicate = snapshot.devices[0].clone();
        duplicate.snapshot.id = DeviceId::from_native("fixture", "duplicate");
        snapshot.devices.push(duplicate);
        target.device = AutomationDeviceReference::Alias {
            alias: "lamp".to_owned(),
        };

        let error = AutomationCompiler::compile(&document(target), &snapshot)
            .expect_err("ambiguous target must fail");
        assert!(error.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::ReferenceAmbiguous
                && finding.path == "/actions/0/target/device"
        }));
    }

    #[test]
    fn invalid_schedule_and_timezone_are_rejected() {
        let (snapshot, target) = fixture();
        let mut document = document(target);
        document.triggers = vec![AutomationTrigger::Schedule {
            schedule: AutomationSchedule {
                cron: "not cron".to_owned(),
                timezone: "Mars/Olympus".to_owned(),
                occurrence_window_ms: 1_000,
            },
        }];

        let error = AutomationCompiler::compile(&document, &snapshot)
            .expect_err("invalid schedule must fail");
        assert!(error.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::InvalidSchedule
                && finding.path == "/triggers/0/schedule"
        }));
    }

    #[test]
    fn stale_incompatible_and_typed_inputs_are_rejected() {
        let (mut snapshot, target) = fixture();
        snapshot.devices[0]
            .transition(LifecycleTrigger::MarkStale)
            .expect("fixture can become stale");
        let stale = AutomationCompiler::compile(&document(target.clone()), &snapshot)
            .expect_err("stale device must fail");
        assert!(stale.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::ReferenceStale
                && finding.path == "/actions/0/target/device"
        }));

        let (snapshot, mut incompatible_target) = fixture();
        incompatible_target.capability = "energy.v1".to_owned();
        let incompatible = AutomationCompiler::compile(&document(incompatible_target), &snapshot)
            .expect_err("incompatible capability must fail");
        assert!(incompatible.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::CapabilityIncompatible
                && finding.path.starts_with("/actions/0")
        }));

        let (snapshot, target) = fixture();
        let mut typed = document(target);
        typed.variables.insert(
            "mode".to_owned(),
            AutomationVariableDefinition {
                value_type: AutomationValueType::String,
                initial: Some(AutomationValue::Integer(1)),
            },
        );
        let typed = AutomationCompiler::compile(&typed, &snapshot)
            .expect_err("mismatched initial value must fail");
        assert!(typed.findings.iter().any(|finding| {
            finding.code == AutomationValidationCode::TypeMismatch
                && finding.path == "/variables/mode/initial"
        }));
    }

    #[test]
    fn position_command_derives_motion_safety_constraints() {
        let (snapshot, mut target) = fixture();
        target.endpoint_id = Some(EndpointId::new("cover"));
        target.capability = "position.v1".to_owned();
        let mut document = document(target.clone());
        document.triggers = vec![AutomationTrigger::ObservationChanged {
            target: target.clone(),
            field: Some("percent".to_owned()),
        }];
        document.actions = vec![AutomationAction::Command {
            target,
            payload: CommandPayload::Position(PositionCommand::GoTo { percent: 42 }),
            retry: retry(),
            on_failure: AutomationFailurePolicy::StopRun,
        }];

        let plan = AutomationCompiler::compile(&document, &snapshot).expect("valid plan");
        assert_eq!(
            plan.safety_profiles,
            BTreeSet::from([AutomationSafetyProfile::ComfortMotion])
        );
        assert!(
            plan.safety_requirements
                .contains(&AutomationSafetyRequirement::FreshState)
        );
        assert!(
            plan.safety_requirements
                .contains(&AutomationSafetyRequirement::StopSupport)
        );
        assert!(
            plan.safety_requirements
                .contains(&AutomationSafetyRequirement::Calibration)
        );
        assert!(
            plan.safety_requirements
                .contains(&AutomationSafetyRequirement::Position)
        );
        assert_eq!(
            plan.approval,
            AutomationApprovalRequirement::ActivationGrant
        );
    }

    fn fixture() -> (FoundationSnapshot, AutomationTargetReference) {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
            .single()
            .expect("valid fixture time");
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, "fixture", "local");
        let device_id = DeviceId::from_integration(&integration_id, "lamp-1");
        let mut record = DeviceRecord::candidate(
            installation_id,
            integration_id,
            DeviceSnapshot {
                id: device_id.clone(),
                native_id: "lamp-1".to_owned(),
                integration: "fixture".to_owned(),
                name: "Living room lamp".to_owned(),
                manufacturer: "Fixture".to_owned(),
                model: "Dual".to_owned(),
                network: Vec::new(),
                endpoints: vec![
                    EndpointSnapshot {
                        id: EndpointId::new("light"),
                        name: Some("Light".to_owned()),
                        capabilities: vec![CapabilitySnapshot::OnOff {
                            on: false,
                            risk: RiskClass::Comfort,
                        }],
                    },
                    EndpointSnapshot {
                        id: EndpointId::new("cover"),
                        name: Some("Cover".to_owned()),
                        capabilities: vec![CapabilitySnapshot::Position {
                            percent: Some(0.0),
                            motion: None,
                            risk: RiskClass::Mechanical,
                        }],
                    },
                ],
                observed_at: now,
                vendor_data: BTreeMap::new(),
            },
            now,
        );
        record.aliases.insert("lamp".to_owned());
        record
            .transition(LifecycleTrigger::Enroll)
            .expect("fixture enrollment");
        (
            FoundationSnapshot {
                devices: vec![record],
                event_cursor: Some(17),
                ..FoundationSnapshot::default()
            },
            AutomationTargetReference {
                device: AutomationDeviceReference::Device { device_id },
                endpoint_id: Some(EndpointId::new("light")),
                capability: "on_off.v1".to_owned(),
            },
        )
    }

    fn document(target: AutomationTargetReference) -> AutomationDocument {
        AutomationDocument {
            schema: AutomationDocumentSchema::V1,
            id: AutomationId::new(),
            version: AutomationVersion::new(1).expect("positive version"),
            name: "Last desired state".to_owned(),
            provenance: AutomationProvenance {
                author_id: ActorId::new(),
                agent_id: Some("test-agent".to_owned()),
                source_request: "Keep the light in the final requested state".to_owned(),
                rationale: "Avoid visible flicker".to_owned(),
            },
            variables: BTreeMap::new(),
            triggers: vec![AutomationTrigger::ObservationChanged {
                target: target.clone(),
                field: Some("on".to_owned()),
            }],
            condition: None,
            actions: vec![AutomationAction::Command {
                target,
                payload: CommandPayload::OnOff(OnOffCommand::Set { on: true }),
                retry: retry(),
                on_failure: AutomationFailurePolicy::StopRun,
            }],
            run_mode: AutomationRunMode::Single,
            self_trigger: AutomationSelfTriggerPolicy::SuppressSameVersion,
            budget: AutomationResourceBudget::default(),
            created_at: Utc
                .with_ymd_and_hms(2026, 7, 11, 12, 0, 0)
                .single()
                .expect("valid fixture time"),
        }
    }

    fn command(target: AutomationTargetReference, on: bool) -> AutomationAction {
        AutomationAction::Command {
            target,
            payload: CommandPayload::OnOff(OnOffCommand::Set { on }),
            retry: retry(),
            on_failure: AutomationFailurePolicy::StopRun,
        }
    }

    fn retry() -> AutomationRetryPolicy {
        AutomationRetryPolicy {
            maximum_retries: 0,
            backoff_ms: 0,
            retryable_command_errors: Vec::<CommandErrorCode>::new(),
        }
    }
}

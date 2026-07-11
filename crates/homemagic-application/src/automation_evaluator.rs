//! Shared deterministic expression and condition evaluation.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use homemagic_domain::{
    AutomationComparison, AutomationValue, ResolvedAutomationCondition,
    ResolvedAutomationExpression, ResolvedAutomationTarget,
};
use thiserror::Error;

/// Immutable values and host-specific duration semantics used by evaluation.
pub trait AutomationEvaluationContext {
    /// Current real or virtual evaluation instant.
    fn now(&self) -> DateTime<Utc>;

    /// Reads one typed normalized observation from an immutable snapshot.
    fn observation(
        &self,
        target: &ResolvedAutomationTarget,
        field: &str,
    ) -> Option<AutomationValue>;

    /// Evaluates a continuously true condition using host-owned time semantics.
    ///
    /// Simulation may advance virtual time. Runtime implementations must turn
    /// the duration into durable waiting state instead of blocking a worker.
    ///
    /// # Errors
    ///
    /// Returns a typed evaluation failure from the nested condition or host
    /// time policy.
    fn state_duration(
        &mut self,
        condition: &ResolvedAutomationCondition,
        duration_ms: u64,
        variables: &BTreeMap<String, AutomationValue>,
    ) -> Result<bool, AutomationEvaluationError>;
}

/// Deterministic evaluation failure shared by simulation and runtime.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AutomationEvaluationError {
    /// A compiled expression referenced unavailable immutable input.
    #[error("automation evaluation value is unavailable: {0}")]
    MissingValue(&'static str),
    /// Operands did not share a comparable compiled type.
    #[error("automation evaluation type mismatch")]
    TypeMismatch,
    /// A compiled timezone or local-time value was invalid.
    #[error("automation evaluation time window is invalid")]
    InvalidTimeWindow,
    /// The host must represent continuous duration as durable waiting state.
    #[error("automation evaluation requires a durable duration wait")]
    DurableDurationRequired,
    /// A durable condition contract could not be canonically hashed.
    #[error("automation evaluation condition hashing failed")]
    ConditionHash,
    /// A duration could not be represented by the runtime clock.
    #[error("automation evaluation duration is outside supported bounds")]
    DurationOverflow,
    /// Persisted continuous-condition state referenced no matching timer.
    #[error("automation evaluation duration timer is unavailable")]
    DurationTimerMissing,
}

/// Evaluates one compiled expression against immutable inputs.
///
/// # Errors
///
/// Returns a typed error when a required value is missing.
pub fn evaluate_automation_expression<C: AutomationEvaluationContext>(
    expression: &ResolvedAutomationExpression,
    variables: &BTreeMap<String, AutomationValue>,
    context: &C,
) -> Result<AutomationValue, AutomationEvaluationError> {
    match expression {
        ResolvedAutomationExpression::Literal { value } => Ok(value.clone()),
        ResolvedAutomationExpression::Variable { name } => variables
            .get(name)
            .cloned()
            .ok_or(AutomationEvaluationError::MissingValue("variable")),
        ResolvedAutomationExpression::Observation { targets, field } => {
            let target = targets
                .first()
                .ok_or(AutomationEvaluationError::MissingValue(
                    "observation target",
                ))?;
            context
                .observation(target, field)
                .ok_or(AutomationEvaluationError::MissingValue("observation"))
        }
    }
}

/// Evaluates one compiled condition using shared short-circuit semantics.
///
/// # Errors
///
/// Returns typed missing-value, type, or time-window failures.
pub fn evaluate_automation_condition<C: AutomationEvaluationContext>(
    condition: &ResolvedAutomationCondition,
    variables: &BTreeMap<String, AutomationValue>,
    context: &mut C,
) -> Result<bool, AutomationEvaluationError> {
    match condition {
        ResolvedAutomationCondition::Literal { value } => Ok(*value),
        ResolvedAutomationCondition::Compare {
            left,
            operator,
            right,
        } => compare_values(
            &evaluate_automation_expression(left, variables, context)?,
            *operator,
            &evaluate_automation_expression(right, variables, context)?,
        ),
        ResolvedAutomationCondition::All { conditions } => {
            for condition in conditions {
                if !evaluate_automation_condition(condition, variables, context)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        ResolvedAutomationCondition::Any { conditions } => {
            for condition in conditions {
                if evaluate_automation_condition(condition, variables, context)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        ResolvedAutomationCondition::Not { condition } => Ok(!evaluate_automation_condition(
            condition, variables, context,
        )?),
        ResolvedAutomationCondition::TimeWindow {
            timezone,
            start,
            end,
        } => evaluate_time_window(context.now(), timezone, start, end),
        ResolvedAutomationCondition::StateDuration {
            condition,
            duration_ms,
        } => context.state_duration(condition, *duration_ms, variables),
    }
}

fn evaluate_time_window(
    now: DateTime<Utc>,
    timezone: &str,
    start: &str,
    end: &str,
) -> Result<bool, AutomationEvaluationError> {
    let timezone =
        Tz::from_str(timezone).map_err(|_| AutomationEvaluationError::InvalidTimeWindow)?;
    let start = NaiveTime::parse_from_str(start, "%H:%M:%S")
        .map_err(|_| AutomationEvaluationError::InvalidTimeWindow)?;
    let end = NaiveTime::parse_from_str(end, "%H:%M:%S")
        .map_err(|_| AutomationEvaluationError::InvalidTimeWindow)?;
    let local = now.with_timezone(&timezone).time();
    Ok(if start <= end {
        local >= start && local < end
    } else {
        local >= start || local < end
    })
}

fn compare_values(
    left: &AutomationValue,
    operator: AutomationComparison,
    right: &AutomationValue,
) -> Result<bool, AutomationEvaluationError> {
    let ordering = match (left, right) {
        (AutomationValue::Null, AutomationValue::Null) => Ordering::Equal,
        (AutomationValue::Boolean(left), AutomationValue::Boolean(right)) => left.cmp(right),
        (AutomationValue::Integer(left), AutomationValue::Integer(right)) => left.cmp(right),
        (AutomationValue::Decimal(left), AutomationValue::Decimal(right)) => left
            .parse::<f64>()
            .ok()
            .and_then(|left| {
                right
                    .parse::<f64>()
                    .ok()
                    .and_then(|right| left.partial_cmp(&right))
            })
            .ok_or(AutomationEvaluationError::TypeMismatch)?,
        (AutomationValue::String(left), AutomationValue::String(right)) => left.cmp(right),
        (AutomationValue::Timestamp(left), AutomationValue::Timestamp(right)) => left.cmp(right),
        (AutomationValue::DurationMillis(left), AutomationValue::DurationMillis(right)) => {
            left.cmp(right)
        }
        _ => return Err(AutomationEvaluationError::TypeMismatch),
    };
    Ok(match operator {
        AutomationComparison::Equal => ordering == Ordering::Equal,
        AutomationComparison::NotEqual => ordering != Ordering::Equal,
        AutomationComparison::LessThan => ordering == Ordering::Less,
        AutomationComparison::LessThanOrEqual => ordering != Ordering::Greater,
        AutomationComparison::GreaterThan => ordering == Ordering::Greater,
        AutomationComparison::GreaterThanOrEqual => ordering != Ordering::Less,
    })
}

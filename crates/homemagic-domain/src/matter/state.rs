use chrono::{DateTime, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::MatterProjectionId;

/// Positive monotonic desired-state revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MatterStateRevision(u64);

impl MatterStateRevision {
    /// Creates a positive desired-state revision.
    ///
    /// # Errors
    ///
    /// Returns [`MatterStateError::ZeroRevision`] for zero.
    pub const fn new(value: u64) -> Result<Self, MatterStateError> {
        if value == 0 {
            Err(MatterStateError::ZeroRevision)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for MatterStateRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Normalized lock state independent from a Matter SDK enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterLockState {
    /// Lock reports secured.
    Locked,
    /// Lock reports unsecured.
    Unlocked,
    /// Lock reports neither stable terminal state.
    NotFullyLocked,
    /// Lock state is not currently known.
    Unknown,
}

/// Bounded state value used by the initial projection contracts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MatterStateValue {
    /// Binary on/off state.
    OnOff(bool),
    /// Normalized level from zero through one hundred.
    LevelPercent(u8),
    /// Normalized position from zero through one hundred.
    PositionPercent(u8),
    /// Door-lock state.
    Lock(MatterLockState),
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum MatterStateValueData {
    OnOff(bool),
    LevelPercent(u8),
    PositionPercent(u8),
    Lock(MatterLockState),
}

impl<'de> Deserialize<'de> for MatterStateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = match MatterStateValueData::deserialize(deserializer)? {
            MatterStateValueData::OnOff(value) => Self::OnOff(value),
            MatterStateValueData::LevelPercent(value) => Self::LevelPercent(value),
            MatterStateValueData::PositionPercent(value) => Self::PositionPercent(value),
            MatterStateValueData::Lock(value) => Self::Lock(value),
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

impl MatterStateValue {
    /// Validates normalized percent-based values.
    ///
    /// # Errors
    ///
    /// Returns [`MatterStateError::PercentOutOfRange`] above one hundred.
    pub const fn validate(&self) -> Result<(), MatterStateError> {
        match self {
            Self::LevelPercent(value) | Self::PositionPercent(value) if *value > 100 => {
                Err(MatterStateError::PercentOutOfRange)
            }
            _ => Ok(()),
        }
    }
}

/// One durable desired state and revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterDesiredState {
    /// Desired-state revision.
    pub revision: MatterStateRevision,
    /// Normalized requested value.
    pub value: MatterStateValue,
    /// Time the desired revision was accepted.
    pub requested_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct MatterDesiredStateData {
    revision: MatterStateRevision,
    value: MatterStateValue,
    requested_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for MatterDesiredState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterDesiredStateData::deserialize(deserializer)?;
        Self::new(data.revision, data.value, data.requested_at).map_err(D::Error::custom)
    }
}

impl MatterDesiredState {
    /// Creates a validated desired state.
    ///
    /// # Errors
    ///
    /// Rejects normalized values outside their capability bounds.
    pub fn new(
        revision: MatterStateRevision,
        value: MatterStateValue,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, MatterStateError> {
        value.validate()?;
        Ok(Self {
            revision,
            value,
            requested_at,
        })
    }
}

/// One normalized reported state from a subscription or bounded read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterReportedState {
    value: MatterStateValue,
    data_version: Option<u32>,
    report_sequence: u64,
    observed_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
}

impl MatterReportedState {
    /// Creates a validated reported state.
    ///
    /// # Errors
    ///
    /// Rejects out-of-range values or an observation after its receipt time.
    pub fn new(
        value: MatterStateValue,
        data_version: Option<u32>,
        report_sequence: u64,
        observed_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Result<Self, MatterStateError> {
        value.validate()?;
        if observed_at > received_at {
            return Err(MatterStateError::ObservationAfterReceipt);
        }
        Ok(Self {
            value,
            data_version,
            report_sequence,
            observed_at,
            received_at,
        })
    }

    /// Returns the normalized reported value.
    #[must_use]
    pub const fn value(&self) -> &MatterStateValue {
        &self.value
    }

    /// Returns the optional Matter data version.
    #[must_use]
    pub const fn data_version(&self) -> Option<u32> {
        self.data_version
    }

    /// Returns the adapter-normalized report sequence.
    #[must_use]
    pub const fn report_sequence(&self) -> u64 {
        self.report_sequence
    }

    /// Returns the source observation time.
    #[must_use]
    pub const fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    /// Returns the local receipt time.
    #[must_use]
    pub const fn received_at(&self) -> DateTime<Utc> {
        self.received_at
    }
}

#[derive(Deserialize)]
struct MatterReportedStateData {
    value: MatterStateValue,
    data_version: Option<u32>,
    report_sequence: u64,
    observed_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for MatterReportedState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterReportedStateData::deserialize(deserializer)?;
        Self::new(
            data.value,
            data.data_version,
            data.report_sequence,
            data.observed_at,
            data.received_at,
        )
        .map_err(D::Error::custom)
    }
}

/// Freshness of one projected Matter state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterStateFreshness {
    /// No trustworthy report has been received.
    Unknown,
    /// The latest report satisfies the projection freshness policy.
    Fresh,
    /// The latest report is older than policy permits.
    Stale,
}

/// Convergence between desired and reported state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterConvergence {
    /// No desired state exists.
    NoDesiredState,
    /// Latest desired state has not yet been confirmed.
    Pending,
    /// Reported state confirmed the latest desired revision.
    Confirmed,
    /// A fresh report differs from the latest desired value.
    Diverged,
    /// A dispatched outcome cannot currently be determined safely.
    Indeterminate,
}

/// Stable reason why projected state is uncertain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterStateUncertainty {
    /// Logical subscription is not currently delivering reports.
    SubscriptionLost,
    /// A notification gap has not yet been closed by a bounded read.
    ReportGap,
    /// Bounded read failed.
    ReadFailed,
    /// Descriptor or projection assumptions changed.
    DescriptorChanged,
    /// A previously dispatched operation has no conclusive report.
    InFlightOutcomeUnknown,
}

/// Complete SDK-neutral desired/reported state projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterProjectedState {
    projection_id: MatterProjectionId,
    desired: Option<MatterDesiredState>,
    reported: Option<MatterReportedState>,
    confirmed_revision: Option<MatterStateRevision>,
    freshness: MatterStateFreshness,
    convergence: MatterConvergence,
    uncertainty: Option<MatterStateUncertainty>,
}

impl MatterProjectedState {
    /// Creates a consistent desired/reported projection.
    ///
    /// # Errors
    ///
    /// Rejects impossible confirmed revisions or convergence states.
    pub fn new(
        projection_id: MatterProjectionId,
        desired: Option<MatterDesiredState>,
        reported: Option<MatterReportedState>,
        confirmed_revision: Option<MatterStateRevision>,
        freshness: MatterStateFreshness,
        convergence: MatterConvergence,
        uncertainty: Option<MatterStateUncertainty>,
    ) -> Result<Self, MatterStateError> {
        if let Some(desired) = &desired {
            desired.value.validate()?;
            if confirmed_revision.is_some_and(|confirmed| confirmed > desired.revision) {
                return Err(MatterStateError::ConfirmedRevisionAhead);
            }
        } else if confirmed_revision.is_some() {
            return Err(MatterStateError::ConfirmationWithoutDesiredState);
        }
        if convergence == MatterConvergence::Confirmed
            && confirmed_revision != desired.as_ref().map(|state| state.revision)
        {
            return Err(MatterStateError::ConfirmedConvergenceMismatch);
        }
        if convergence == MatterConvergence::NoDesiredState && desired.is_some() {
            return Err(MatterStateError::DesiredStateConvergenceMismatch);
        }
        Ok(Self {
            projection_id,
            desired,
            reported,
            confirmed_revision,
            freshness,
            convergence,
            uncertainty,
        })
    }

    /// Returns the stable projection identity.
    #[must_use]
    pub const fn projection_id(&self) -> &MatterProjectionId {
        &self.projection_id
    }

    /// Returns the latest desired state.
    #[must_use]
    pub const fn desired(&self) -> Option<&MatterDesiredState> {
        self.desired.as_ref()
    }

    /// Returns the latest reported state.
    #[must_use]
    pub const fn reported(&self) -> Option<&MatterReportedState> {
        self.reported.as_ref()
    }

    /// Returns the latest confirmed desired revision.
    #[must_use]
    pub const fn confirmed_revision(&self) -> Option<MatterStateRevision> {
        self.confirmed_revision
    }

    /// Returns current freshness.
    #[must_use]
    pub const fn freshness(&self) -> MatterStateFreshness {
        self.freshness
    }

    /// Returns current convergence.
    #[must_use]
    pub const fn convergence(&self) -> MatterConvergence {
        self.convergence
    }

    /// Returns the structured uncertainty reason.
    #[must_use]
    pub const fn uncertainty(&self) -> Option<MatterStateUncertainty> {
        self.uncertainty
    }
}

#[derive(Deserialize)]
struct MatterProjectedStateData {
    projection_id: MatterProjectionId,
    desired: Option<MatterDesiredState>,
    reported: Option<MatterReportedState>,
    confirmed_revision: Option<MatterStateRevision>,
    freshness: MatterStateFreshness,
    convergence: MatterConvergence,
    uncertainty: Option<MatterStateUncertainty>,
}

impl<'de> Deserialize<'de> for MatterProjectedState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterProjectedStateData::deserialize(deserializer)?;
        Self::new(
            data.projection_id,
            data.desired,
            data.reported,
            data.confirmed_revision,
            data.freshness,
            data.convergence,
            data.uncertainty,
        )
        .map_err(D::Error::custom)
    }
}

/// Invalid projected Matter state.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterStateError {
    /// Desired-state revisions start at one.
    #[error("Matter desired-state revision must be non-zero")]
    ZeroRevision,
    /// A normalized percent exceeded one hundred.
    #[error("Matter normalized percent must be between zero and one hundred")]
    PercentOutOfRange,
    /// Source observation time was after local receipt.
    #[error("Matter observation time must not be after receipt time")]
    ObservationAfterReceipt,
    /// Confirmed revision exceeded the current desired revision.
    #[error("Matter confirmed revision cannot exceed desired revision")]
    ConfirmedRevisionAhead,
    /// A confirmation existed without desired state.
    #[error("Matter state cannot be confirmed without desired state")]
    ConfirmationWithoutDesiredState,
    /// Confirmed convergence did not match the latest desired revision.
    #[error("Matter confirmed convergence must match the latest desired revision")]
    ConfirmedConvergenceMismatch,
    /// No-desired-state convergence was paired with desired state.
    #[error("Matter no-desired-state convergence cannot contain desired state")]
    DesiredStateConvergenceMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatterFabricId;

    fn projection_id() -> MatterProjectionId {
        MatterProjectionId::from_key(&MatterFabricId::new(), 42, 1, "on_off", 1)
    }

    #[test]
    fn state_revision_should_reject_zero_during_deserialization() {
        let result = serde_json::from_str::<MatterStateRevision>("0");

        assert!(result.is_err(), "zero revision should be rejected");
    }

    #[test]
    fn state_value_should_reject_out_of_range_percent_during_deserialization() {
        let result =
            serde_json::from_str::<MatterStateValue>(r#"{"type":"level_percent","value":101}"#);

        assert!(result.is_err(), "out-of-range percent should be rejected");
    }

    #[test]
    fn reported_state_should_reject_observation_after_receipt() {
        let received_at = Utc::now();
        let observed_at = received_at + chrono::Duration::seconds(1);

        let result = MatterReportedState::new(
            MatterStateValue::OnOff(true),
            Some(1),
            1,
            observed_at,
            received_at,
        );

        assert_eq!(result, Err(MatterStateError::ObservationAfterReceipt));
    }

    #[test]
    fn projected_state_should_reject_confirmation_ahead_of_desired() -> Result<(), MatterStateError>
    {
        let now = Utc::now();
        let desired = MatterDesiredState::new(
            MatterStateRevision::new(1)?,
            MatterStateValue::OnOff(true),
            now,
        )?;

        let result = MatterProjectedState::new(
            projection_id(),
            Some(desired),
            None,
            Some(MatterStateRevision::new(2)?),
            MatterStateFreshness::Unknown,
            MatterConvergence::Pending,
            None,
        );

        assert_eq!(result, Err(MatterStateError::ConfirmedRevisionAhead));
        Ok(())
    }

    #[test]
    fn projected_state_should_round_trip_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let state = MatterProjectedState::new(
            projection_id(),
            None,
            None,
            None,
            MatterStateFreshness::Unknown,
            MatterConvergence::NoDesiredState,
            Some(MatterStateUncertainty::SubscriptionLost),
        )?;

        let encoded = serde_json::to_string(&state)?;
        let decoded = serde_json::from_str(&encoded)?;

        assert_eq!(state, decoded);
        Ok(())
    }
}

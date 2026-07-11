use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{CapabilityDescriptor, DeviceId, EndpointId, IntegrationId};

/// One field value with the time reported by its source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservedValue {
    /// JSON-compatible value defined by the capability schema.
    pub value: Value,
    /// Source timestamp for this field.
    pub observed_at: DateTime<Utc>,
}

/// Kind of adapter message that produced an observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSourceKind {
    /// Complete status snapshot.
    FullStatus,
    /// Partial status notification.
    Notification,
    /// Device-originated event frame.
    Event,
    /// Bounded refresh after a subscription gap.
    RefreshFallback,
}

/// Adapter provenance for an observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservationSource {
    /// Integration instance that produced the data.
    pub integration_id: IntegrationId,
    /// Protocol message kind.
    pub kind: ObservationSourceKind,
    /// Optional adapter-native sequence number.
    pub sequence: Option<u64>,
}

/// Versioned capability observation with independently timestamped fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityObservation {
    /// Stable device target.
    pub device_id: DeviceId,
    /// Stable endpoint target.
    pub endpoint_id: EndpointId,
    /// Versioned capability contract.
    pub capability: CapabilityDescriptor,
    /// Current fields keyed by schema-defined name.
    pub values: BTreeMap<String, ObservedValue>,
    /// Time `HomeMagic` received this message.
    pub received_at: DateTime<Utc>,
    /// Adapter provenance.
    pub source: ObservationSource,
}

impl CapabilityObservation {
    /// Merges a partial observation without dropping unchanged fields.
    ///
    /// Older values are ignored per field. The returned field names are the
    /// values that changed or were added.
    ///
    /// # Errors
    ///
    /// Returns an error when device, endpoint, capability, or integration
    /// provenance differs.
    pub fn merge_partial(&mut self, patch: Self) -> Result<Vec<String>, ObservationMergeError> {
        if self.device_id != patch.device_id
            || self.endpoint_id != patch.endpoint_id
            || self.capability != patch.capability
            || self.source.integration_id != patch.source.integration_id
        {
            return Err(ObservationMergeError::TargetMismatch);
        }

        let mut changed = Vec::new();
        for (field, incoming) in patch.values {
            let should_replace = self
                .values
                .get(&field)
                .is_none_or(|existing| incoming.observed_at >= existing.observed_at);
            if should_replace {
                let value_changed = self.values.get(&field) != Some(&incoming);
                self.values.insert(field.clone(), incoming);
                if value_changed {
                    changed.push(field);
                }
            }
        }
        if !changed.is_empty() {
            self.received_at = self.received_at.max(patch.received_at);
            self.source = patch.source;
        }
        Ok(changed)
    }
}

/// Partial observation addressed a different capability target.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMergeError {
    /// Target identity, descriptor, or integration did not match.
    #[error("partial observation target does not match current observation")]
    TargetMismatch,
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::json;

    use super::*;
    use crate::{InstallationId, RiskClass};

    fn observation(
        integration: &IntegrationId,
        values: BTreeMap<String, ObservedValue>,
    ) -> Result<CapabilityObservation, crate::CapabilityDescriptorError> {
        Ok(CapabilityObservation {
            device_id: DeviceId::from_integration(integration, "native"),
            endpoint_id: EndpointId::new("switch:0"),
            capability: CapabilityDescriptor::new("on_off", 1, RiskClass::Comfort)?,
            values,
            received_at: Utc::now(),
            source: ObservationSource {
                integration_id: integration.clone(),
                kind: ObservationSourceKind::Notification,
                sequence: Some(1),
            },
        })
    }

    #[test]
    fn partial_merge_should_preserve_unchanged_fields() -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let installation = InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "test", "local");
        let mut current = observation(
            &integration,
            BTreeMap::from([
                (
                    "on".to_owned(),
                    ObservedValue {
                        value: json!(false),
                        observed_at: now,
                    },
                ),
                (
                    "watts".to_owned(),
                    ObservedValue {
                        value: json!(12.0),
                        observed_at: now,
                    },
                ),
            ]),
        )?;
        let patch = observation(
            &integration,
            BTreeMap::from([(
                "on".to_owned(),
                ObservedValue {
                    value: json!(true),
                    observed_at: now,
                },
            )]),
        )?;

        current.merge_partial(patch)?;

        assert_eq!(
            current.values["watts"],
            ObservedValue {
                value: json!(12.0),
                observed_at: now,
            }
        );
        Ok(())
    }

    #[test]
    fn partial_merge_should_ignore_older_field_value() -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let installation = InstallationId::new();
        let integration = IntegrationId::from_native(&installation, "test", "local");
        let mut current = observation(
            &integration,
            BTreeMap::from([(
                "on".to_owned(),
                ObservedValue {
                    value: json!(true),
                    observed_at: now,
                },
            )]),
        )?;
        let patch = observation(
            &integration,
            BTreeMap::from([(
                "on".to_owned(),
                ObservedValue {
                    value: json!(false),
                    observed_at: now - chrono::TimeDelta::seconds(1),
                },
            )]),
        )?;

        let changed = current.merge_partial(patch)?;

        assert!(changed.is_empty());
        Ok(())
    }
}

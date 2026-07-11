use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

const DEFAULT_EVENT_WINDOW: usize = 256;

/// Parsed Shelly notification frame.
#[derive(Clone, Debug, PartialEq)]
pub enum NotificationFrame {
    /// Partial or complete device status.
    Status(StatusNotification),
    /// One or more device-originated events.
    Events(Vec<ShellyEvent>),
}

/// Status overlay emitted by `NotifyStatus` or `NotifyFullStatus`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusNotification {
    /// Device-provided Unix timestamp.
    pub timestamp: f64,
    /// Whether this frame replaces the complete baseline.
    pub full: bool,
    /// Component payloads keyed by `component[:id]`.
    pub components: BTreeMap<String, Value>,
}

/// One raw, secret-free Shelly event.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellyEvent {
    /// Component key such as `input:0`.
    pub component: String,
    /// Stable event name such as `single_push`.
    pub event: String,
    /// Device-provided Unix timestamp.
    pub timestamp: f64,
    /// Remaining vendor fields retained for later capability projection.
    pub data: BTreeMap<String, Value>,
}

/// Secret-safe notification parsing failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum NotificationError {
    /// Frame is not valid JSON.
    #[error("notification frame is not valid JSON")]
    InvalidJson,
    /// Required RPC envelope fields are absent or invalid.
    #[error("notification frame has an invalid envelope")]
    InvalidEnvelope,
    /// Notification method is not supported by the device foundation.
    #[error("notification method is unsupported")]
    UnsupportedMethod,
    /// Status parameters are malformed.
    #[error("status notification parameters are invalid")]
    InvalidStatus,
    /// Event parameters are malformed.
    #[error("event notification parameters are invalid")]
    InvalidEvent,
}

#[derive(Deserialize)]
struct Envelope {
    src: String,
    dst: String,
    method: String,
    params: Value,
}

/// Parses a Shelly RPC notification without retaining envelope credentials.
///
/// # Errors
///
/// Returns a stable parsing error for malformed or unsupported frames.
pub fn parse_notification(frame: &str) -> Result<NotificationFrame, NotificationError> {
    let envelope: Envelope =
        serde_json::from_str(frame).map_err(|_| NotificationError::InvalidJson)?;
    if envelope.src.trim().is_empty() || envelope.dst.trim().is_empty() {
        return Err(NotificationError::InvalidEnvelope);
    }
    match envelope.method.as_str() {
        "NotifyStatus" => parse_status(envelope.params, false),
        "NotifyFullStatus" => parse_status(envelope.params, true),
        "NotifyEvent" => parse_events(envelope.params),
        _ => Err(NotificationError::UnsupportedMethod),
    }
}

fn parse_status(params: Value, full: bool) -> Result<NotificationFrame, NotificationError> {
    let Value::Object(mut params) = params else {
        return Err(NotificationError::InvalidStatus);
    };
    let timestamp = params
        .remove("ts")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite())
        .ok_or(NotificationError::InvalidStatus)?;
    if params.values().any(|value| !value.is_object()) {
        return Err(NotificationError::InvalidStatus);
    }
    Ok(NotificationFrame::Status(StatusNotification {
        timestamp,
        full,
        components: params.into_iter().collect(),
    }))
}

fn parse_events(params: Value) -> Result<NotificationFrame, NotificationError> {
    let Value::Object(mut params) = params else {
        return Err(NotificationError::InvalidEvent);
    };
    let Value::Array(events) = params
        .remove("events")
        .ok_or(NotificationError::InvalidEvent)?
    else {
        return Err(NotificationError::InvalidEvent);
    };
    let events = events
        .into_iter()
        .map(|event| {
            let Value::Object(mut event) = event else {
                return Err(NotificationError::InvalidEvent);
            };
            let component = take_string(&mut event, "component")?;
            let event_name = take_string(&mut event, "event")?;
            let timestamp = event
                .remove("ts")
                .and_then(|value| value.as_f64())
                .filter(|value| value.is_finite())
                .ok_or(NotificationError::InvalidEvent)?;
            Ok(ShellyEvent {
                component,
                event: event_name,
                timestamp,
                data: event.into_iter().collect(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NotificationFrame::Events(events))
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Result<String, NotificationError> {
    object
        .remove(key)
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
        .ok_or(NotificationError::InvalidEvent)
}

/// Result of applying one status notification to the session cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusApply {
    /// Components whose effective status changed.
    pub changed_components: Vec<String>,
    /// Whether a full refresh is required before more patches are trusted.
    pub requires_refresh: bool,
}

/// Per-session complete status baseline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusCache {
    components: BTreeMap<String, Value>,
    last_timestamp: Option<f64>,
}

impl StatusCache {
    /// Returns the effective complete component status.
    #[must_use]
    pub const fn components(&self) -> &BTreeMap<String, Value> {
        &self.components
    }

    /// Overlays a partial notification or replaces the full baseline.
    #[must_use]
    pub fn apply(&mut self, notification: StatusNotification) -> StatusApply {
        if self
            .last_timestamp
            .is_some_and(|accepted| notification.timestamp < accepted)
        {
            return StatusApply {
                changed_components: Vec::new(),
                requires_refresh: true,
            };
        }
        let before = self.components.clone();
        if notification.full {
            self.components = notification.components;
        } else {
            for (component, patch) in notification.components {
                match self.components.get_mut(&component) {
                    Some(current) => merge_value(current, patch),
                    None => {
                        self.components.insert(component, patch);
                    }
                }
            }
        }
        self.last_timestamp = Some(notification.timestamp);
        let changed_components = self
            .components
            .iter()
            .filter(|(component, value)| before.get(*component) != Some(*value))
            .map(|(component, _)| component.clone())
            .chain(
                before
                    .keys()
                    .filter(|component| !self.components.contains_key(*component))
                    .cloned(),
            )
            .collect();
        StatusApply {
            changed_components,
            requires_refresh: false,
        }
    }
}

fn merge_value(current: &mut Value, patch: Value) {
    match (current, patch) {
        (Value::Object(current), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    current.remove(&key);
                } else if let Some(existing) = current.get_mut(&key) {
                    merge_value(existing, value);
                } else {
                    current.insert(key, value);
                }
            }
        }
        (current, patch) => *current = patch,
    }
}

/// Bounded replay filter for device-originated event frames.
#[derive(Clone, Debug)]
pub struct EventDeduplicator {
    capacity: usize,
    order: VecDeque<String>,
    fingerprints: BTreeSet<String>,
}

impl Default for EventDeduplicator {
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_WINDOW)
    }
}

impl EventDeduplicator {
    /// Creates a replay window with at least one retained event.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            fingerprints: BTreeSet::new(),
        }
    }

    /// Returns `true` once for each event fingerprint retained in the window.
    pub fn accept(&mut self, event: &ShellyEvent) -> bool {
        let fingerprint = event_fingerprint(event);
        if !self.fingerprints.insert(fingerprint.clone()) {
            return false;
        }
        self.order.push_back(fingerprint);
        while self.order.len() > self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.fingerprints.remove(&expired);
            }
        }
        true
    }
}

fn event_fingerprint(event: &ShellyEvent) -> String {
    let data = serde_json::to_string(&event.data).unwrap_or_default();
    format!(
        "{}:{}:{}:{data}",
        event.component, event.event, event.timestamp
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_fixture(path: &str) -> StatusNotification {
        let frame = match path {
            "full" => include_str!("../tests/fixtures/notify_full_status.json"),
            "partial" => include_str!("../tests/fixtures/notify_status_partial.json"),
            _ => panic!("unknown fixture"),
        };
        let NotificationFrame::Status(status) =
            parse_notification(frame).unwrap_or_else(|error| panic!("status fixture: {error}"))
        else {
            panic!("expected status notification");
        };
        status
    }

    #[test]
    fn partial_status_should_preserve_unchanged_fields_and_remove_nulls() {
        let mut cache = StatusCache::default();
        let _ = cache.apply(status_fixture("full"));

        let outcome = cache.apply(status_fixture("partial"));
        let switch = cache.components()["switch:0"]
            .as_object()
            .unwrap_or_else(|| panic!("switch status object"));

        assert_eq!(outcome.changed_components, vec!["switch:0"]);
        assert_eq!(switch.get("output"), Some(&Value::Bool(true)));
        assert_eq!(switch.get("apower"), Some(&Value::from(12.5)));
        assert!(!switch.contains_key("temperature"));
    }

    #[test]
    fn identical_status_should_be_idempotent() {
        let mut cache = StatusCache::default();
        let full = status_fixture("full");
        assert!(!cache.apply(full.clone()).changed_components.is_empty());
        assert!(cache.apply(full).changed_components.is_empty());
    }

    #[test]
    fn older_status_should_request_refresh_without_mutation() {
        let mut cache = StatusCache::default();
        let _ = cache.apply(status_fixture("full"));
        let before = cache.clone();
        let mut older = status_fixture("partial");
        older.timestamp = 1.0;

        let outcome = cache.apply(older);

        assert!(outcome.requires_refresh);
        assert_eq!(cache, before);
    }

    #[test]
    fn replayed_event_should_be_deduplicated() {
        let NotificationFrame::Events(events) =
            parse_notification(include_str!("../tests/fixtures/notify_event.json"))
                .unwrap_or_else(|error| panic!("event fixture: {error}"))
        else {
            panic!("expected event notification");
        };
        let mut deduplicator = EventDeduplicator::default();

        assert!(deduplicator.accept(&events[0]));
        assert!(!deduplicator.accept(&events[0]));
    }

    #[test]
    fn malformed_frame_should_return_stable_error() {
        assert_eq!(
            parse_notification(include_str!("../tests/fixtures/notify_malformed.json")),
            Err(NotificationError::InvalidJson)
        );
    }
}

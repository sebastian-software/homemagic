//! Shelly Gen2+ local integration.
//!
//! The initial adapter discovers `_shelly._tcp.local.` services and reads the
//! public device information and current status through Shelly HTTP RPC.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use homemagic_application::{BoxError, IntegrationScanner};
use homemagic_domain::{
    CapabilitySnapshot, DeviceId, DeviceSnapshot, DiscoveryCandidate, EndpointId, EndpointSnapshot,
    InstallationId, IntegrationId, NetworkLocation, RiskClass,
};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{debug, warn};

const INTEGRATION: &str = "shelly";
const SHELLY_SERVICE_TYPE: &str = "_shelly._tcp.local.";
const HTTP_SERVICE_TYPE: &str = "_http._tcp.local.";

/// Shelly Gen2+ device identity returned by `Shelly.GetDeviceInfo`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShellyDeviceInfo {
    /// Manufacturer-native device identifier.
    pub id: String,
    /// Hardware MAC address.
    pub mac: String,
    /// Manufacturer model identifier.
    pub model: String,
    /// Shelly hardware/API generation.
    #[serde(rename = "gen")]
    pub generation: u8,
    /// Full firmware build identifier.
    #[serde(default)]
    pub fw_id: Option<String>,
    /// Firmware version.
    #[serde(default)]
    pub ver: Option<String>,
    /// Shelly application name.
    #[serde(default)]
    pub app: Option<String>,
    /// Active device profile, when applicable.
    #[serde(default)]
    pub profile: Option<String>,
    /// Whether HTTP RPC requires authentication.
    #[serde(default)]
    pub auth_en: bool,
}

/// One resolved Shelly mDNS service.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DiscoveredShelly {
    /// Resolved address.
    pub address: IpAddr,
    /// Resolved HTTP RPC port.
    pub port: u16,
}

/// Shelly adapter failure.
#[derive(Debug, Error)]
pub enum ShellyError {
    /// mDNS daemon or browse failure.
    #[error("mDNS discovery failed: {0}")]
    Discovery(String),
    /// A blocking discovery worker failed unexpectedly.
    #[error("mDNS discovery worker failed: {0}")]
    DiscoveryWorker(String),
    /// The macOS native discovery fallback failed.
    #[cfg(target_os = "macos")]
    #[error("macOS native discovery failed: {0}")]
    NativeDiscovery(String),
    /// HTTP transport failure.
    #[error("request to {url} failed: {source}")]
    Http {
        /// Requested URL.
        url: String,
        /// HTTP client error.
        source: reqwest::Error,
    },
    /// Device requires credentials not yet configured in this prototype.
    #[error("device at {host} requires authentication")]
    AuthenticationRequired {
        /// Device address.
        host: String,
    },
    /// Shelly returned an unexpected HTTP status.
    #[error("device at {host} returned HTTP {status}")]
    HttpStatus {
        /// Device address.
        host: String,
        /// Returned status.
        status: StatusCode,
    },
}

/// Read-only scanner for local Shelly Gen2+ devices.
#[derive(Clone)]
pub struct ShellyScanner {
    client: Client,
    discovery_window: Duration,
    installation_id: InstallationId,
    integration_id: IntegrationId,
}

impl ShellyScanner {
    /// Creates a scanner with the given mDNS collection window.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be constructed.
    pub fn new(discovery_window: Duration) -> Result<Self, reqwest::Error> {
        let installation_id = InstallationId::new();
        let integration_id = IntegrationId::from_native(&installation_id, INTEGRATION, "local");
        Self::with_identity(discovery_window, installation_id, integration_id)
    }

    /// Creates a scanner bound to durable installation and integration IDs.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be constructed.
    pub fn with_identity(
        discovery_window: Duration,
        installation_id: InstallationId,
        integration_id: IntegrationId,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder().timeout(Duration::from_secs(4)).build()?;
        Ok(Self {
            client,
            discovery_window,
            installation_id,
            integration_id,
        })
    }

    async fn fetch_snapshot(
        &self,
        target: &DiscoveredShelly,
    ) -> Result<DeviceSnapshot, ShellyError> {
        let info_url = rpc_url(target, "Shelly.GetDeviceInfo");
        let info = self.get_json::<ShellyDeviceInfo>(&info_url).await?;
        let status_url = rpc_url(target, "Shelly.GetStatus");
        let config_url = rpc_url(target, "Shelly.GetConfig");
        let (status, config) = tokio::join!(
            self.get_json::<Map<String, Value>>(&status_url),
            self.get_json::<Map<String, Value>>(&config_url),
        );
        let status = match status {
            Ok(value) => Some(value),
            Err(ShellyError::AuthenticationRequired { .. }) => {
                warn!(device = %info.id, "Shelly status requires authentication");
                None
            }
            Err(error) => return Err(error),
        };
        let config = match config {
            Ok(value) => Some(value),
            Err(ShellyError::AuthenticationRequired { .. }) => None,
            Err(error) => return Err(error),
        };

        Ok(project_snapshot(
            target,
            &self.integration_id,
            info,
            status,
            config,
        ))
    }

    async fn get_json<T>(&self, url: &str) -> Result<T, ShellyError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|source| ShellyError::Http {
                url: url.to_owned(),
                source,
            })?;

        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ShellyError::AuthenticationRequired {
                host: url.to_owned(),
            });
        }
        if !response.status().is_success() {
            return Err(ShellyError::HttpStatus {
                host: url.to_owned(),
                status: response.status(),
            });
        }

        response.json().await.map_err(|source| ShellyError::Http {
            url: url.to_owned(),
            source,
        })
    }
}

#[async_trait]
impl IntegrationScanner for ShellyScanner {
    fn integration(&self) -> &'static str {
        INTEGRATION
    }

    async fn scan(&self) -> Result<Vec<DiscoveryCandidate>, BoxError> {
        let targets = discover(self.discovery_window).await?;
        let mut snapshots = BTreeMap::new();
        let mut tasks = JoinSet::new();

        for target in targets {
            let scanner = self.clone();
            tasks.spawn(async move {
                let result = scanner.fetch_snapshot(&target).await;
                (target, result)
            });
        }

        while let Some(task) = tasks.join_next().await {
            match task {
                Ok((_, Ok(snapshot))) => {
                    snapshots.insert(snapshot.id.clone(), snapshot);
                }
                Ok((target, Err(error))) => {
                    warn!(address = %target.address, %error, "Shelly refresh failed");
                }
                Err(error) => warn!(%error, "Shelly refresh task failed"),
            }
        }

        Ok(snapshots
            .into_values()
            .map(|snapshot| DiscoveryCandidate {
                installation_id: self.installation_id.clone(),
                integration_id: self.integration_id.clone(),
                discovered_at: snapshot.observed_at,
                snapshot,
            })
            .collect())
    }
}

/// Discovers local Shelly Gen2+ services for a bounded time window.
///
/// # Errors
///
/// Returns an error if the mDNS daemon cannot start or browse.
pub async fn discover(window: Duration) -> Result<Vec<DiscoveredShelly>, ShellyError> {
    let discovered = tokio::task::spawn_blocking(move || discover_blocking(window))
        .await
        .map_err(|error| ShellyError::DiscoveryWorker(error.to_string()))??;

    #[cfg(target_os = "macos")]
    if discovered.is_empty() {
        debug!("pure Rust mDNS returned no services; trying macOS mDNSResponder");
        return discover_native_macos(window).await;
    }

    Ok(discovered)
}

#[cfg(target_os = "macos")]
async fn discover_native_macos(window: Duration) -> Result<Vec<DiscoveredShelly>, ShellyError> {
    let mut child = tokio::process::Command::new("/usr/bin/dns-sd")
        .args(["-Z", "_http._tcp", "local."])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ShellyError::NativeDiscovery(error.to_string()))?;

    tokio::time::sleep(window).await;
    child
        .start_kill()
        .map_err(|error| ShellyError::NativeDiscovery(error.to_string()))?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| ShellyError::NativeDiscovery(error.to_string()))?;
    let services = parse_dns_sd_zone(&String::from_utf8_lossy(&output.stdout));
    let mut discovered = BTreeSet::new();
    let mut tasks = JoinSet::new();

    for service in services {
        tasks.spawn(async move {
            let result =
                match tokio::net::lookup_host((service.hostname.as_str(), service.port)).await {
                    Ok(addresses) => Ok(addresses.collect::<Vec<_>>()),
                    Err(error) => Err(error),
                };
            (service, result)
        });
    }

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok((_, Ok(addresses))) => {
                for socket in addresses {
                    discovered.insert(DiscoveredShelly {
                        address: socket.ip(),
                        port: socket.port(),
                    });
                }
            }
            Ok((service, Err(error))) => {
                warn!(host = service.hostname, %error, "could not resolve Shelly mDNS host");
            }
            Err(error) => warn!(%error, "Shelly hostname resolution task failed"),
        }
    }

    Ok(prefer_ipv4(discovered))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NativeService {
    hostname: String,
    port: u16,
}

#[cfg(target_os = "macos")]
fn parse_dns_sd_zone(output: &str) -> BTreeSet<NativeService> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 6
                || !fields[0].to_ascii_lowercase().starts_with("shelly")
                || fields[1] != "SRV"
            {
                return None;
            }
            let port = fields[4].parse().ok()?;
            Some(NativeService {
                hostname: fields[5].trim_end_matches('.').to_owned(),
                port,
            })
        })
        .collect()
}

fn discover_blocking(window: Duration) -> Result<Vec<DiscoveredShelly>, ShellyError> {
    let shelly_services = discover_service(SHELLY_SERVICE_TYPE, window, false)?;
    if !shelly_services.is_empty() {
        return Ok(prefer_ipv4(shelly_services));
    }

    debug!("no dedicated Shelly services resolved; trying HTTP service fallback");
    discover_service(HTTP_SERVICE_TYPE, window, true).map(prefer_ipv4)
}

fn discover_service(
    service_type: &str,
    window: Duration,
    filter_shelly: bool,
) -> Result<BTreeSet<DiscoveredShelly>, ShellyError> {
    let daemon = ServiceDaemon::new().map_err(discovery_error)?;
    let receiver = daemon.browse(service_type).map_err(discovery_error)?;
    let deadline = Instant::now() + window;
    let mut discovered = BTreeSet::new();

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if remaining.is_zero() {
            break;
        }

        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };

        if let ServiceEvent::ServiceResolved(info) = event {
            if filter_shelly && !is_shelly_http_service(&info) {
                continue;
            }
            debug!(service = info.get_fullname(), "resolved Shelly service");
            let port = info.get_port();
            for address in info.get_addresses() {
                discovered.insert(DiscoveredShelly {
                    address: address.to_ip_addr(),
                    port,
                });
            }
        }
    }

    if let Err(error) = daemon.stop_browse(service_type) {
        debug!(%error, "failed to stop Shelly mDNS browse cleanly");
    }
    if let Err(error) = daemon.shutdown() {
        debug!(%error, "failed to shut down mDNS daemon cleanly");
    }

    Ok(discovered)
}

fn is_shelly_http_service(info: &mdns_sd::ResolvedService) -> bool {
    matches!(info.get_property_val_str("gen"), Some("2" | "3" | "4"))
        || info
            .get_fullname()
            .to_ascii_lowercase()
            .starts_with("shelly")
}

fn prefer_ipv4(discovered: BTreeSet<DiscoveredShelly>) -> Vec<DiscoveredShelly> {
    let ipv4: Vec<_> = discovered
        .iter()
        .filter(|target| target.address.is_ipv4())
        .cloned()
        .collect();
    if ipv4.is_empty() {
        discovered.into_iter().collect()
    } else {
        ipv4
    }
}

fn discovery_error(error: impl std::fmt::Display) -> ShellyError {
    ShellyError::Discovery(error.to_string())
}

fn rpc_url(target: &DiscoveredShelly, method: &str) -> String {
    let socket = SocketAddr::new(target.address, target.port);
    format!("http://{socket}/rpc/{method}")
}

fn project_snapshot(
    target: &DiscoveredShelly,
    integration_id: &IntegrationId,
    info: ShellyDeviceInfo,
    status: Option<Map<String, Value>>,
    config: Option<Map<String, Value>>,
) -> DeviceSnapshot {
    let mut endpoints = vec![EndpointSnapshot {
        id: EndpointId::new("device"),
        name: None,
        capabilities: vec![
            CapabilitySnapshot::Availability { online: true },
            CapabilitySnapshot::Diagnostics {
                firmware_version: info.ver.clone(),
                errors: Vec::new(),
            },
        ],
    }];

    if let Some(status) = &status {
        endpoints.extend(project_components(status));
    }

    let mut vendor_data = BTreeMap::new();
    vendor_data.insert(
        "shelly.device_info".to_owned(),
        serde_json::to_value(&info).unwrap_or(Value::Null),
    );
    if let Some(status) = status {
        vendor_data.insert("shelly.status".to_owned(), Value::Object(status));
    } else if info.auth_en {
        vendor_data.insert(
            "shelly.authentication_required".to_owned(),
            Value::Bool(true),
        );
    }
    let name = config
        .as_ref()
        .and_then(device_name)
        .unwrap_or_else(|| info.id.clone());
    if let Some(config) = config {
        vendor_data.insert("shelly.config".to_owned(), Value::Object(config));
    }

    DeviceSnapshot {
        id: DeviceId::from_integration(integration_id, &info.id),
        native_id: info.id.clone(),
        integration: INTEGRATION.to_owned(),
        name,
        manufacturer: "Shelly".to_owned(),
        model: info.model,
        network: vec![NetworkLocation {
            host: target.address.to_string(),
            port: target.port,
        }],
        endpoints,
        observed_at: Utc::now(),
        vendor_data,
    }
}

fn device_name(config: &Map<String, Value>) -> Option<String> {
    config
        .get("sys")?
        .as_object()?
        .get("device")?
        .as_object()?
        .get("name")?
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn project_components(status: &Map<String, Value>) -> Vec<EndpointSnapshot> {
    status
        .iter()
        .filter_map(|(key, value)| {
            let (kind, _) = key.split_once(':')?;
            let component = value.as_object()?;
            let capabilities = match kind {
                "switch" => project_switch(component),
                "light" => project_light(component),
                "cover" => project_cover(component),
                _ => return None,
            };
            Some(EndpointSnapshot {
                id: EndpointId::new(key),
                name: None,
                capabilities,
            })
        })
        .collect()
}

fn project_switch(component: &Map<String, Value>) -> Vec<CapabilitySnapshot> {
    let mut capabilities = Vec::new();
    if let Some(on) = boolean(component, "output") {
        capabilities.push(CapabilitySnapshot::OnOff {
            on,
            risk: RiskClass::Comfort,
        });
    }
    append_power_and_energy(component, &mut capabilities);
    capabilities
}

fn project_light(component: &Map<String, Value>) -> Vec<CapabilitySnapshot> {
    let mut capabilities = project_switch(component);
    if let Some(percent) = number(component, "brightness") {
        capabilities.push(CapabilitySnapshot::Level {
            percent,
            risk: RiskClass::Comfort,
        });
    }
    capabilities
}

fn project_cover(component: &Map<String, Value>) -> Vec<CapabilitySnapshot> {
    let mut capabilities = vec![CapabilitySnapshot::Position {
        percent: number(component, "current_pos"),
        motion: string(component, "state").map(ToOwned::to_owned),
        risk: RiskClass::Mechanical,
    }];
    append_power_and_energy(component, &mut capabilities);
    capabilities
}

fn append_power_and_energy(
    component: &Map<String, Value>,
    capabilities: &mut Vec<CapabilitySnapshot>,
) {
    let watts = number(component, "apower");
    let volts = number(component, "voltage");
    let amperes = number(component, "current");
    if watts.is_some() || volts.is_some() || amperes.is_some() {
        capabilities.push(CapabilitySnapshot::Power {
            watts,
            volts,
            amperes,
        });
    }
    if let Some(watt_hours) = component
        .get("aenergy")
        .and_then(Value::as_object)
        .and_then(|energy| number(energy, "total"))
    {
        capabilities.push(CapabilitySnapshot::Energy { watt_hours });
    }
}

fn boolean(component: &Map<String, Value>, key: &str) -> Option<bool> {
    component.get(key).and_then(Value::as_bool)
}

fn number(component: &Map<String, Value>, key: &str) -> Option<f64> {
    component.get(key).and_then(Value::as_f64)
}

fn string<'a>(component: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    component.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_info() -> ShellyDeviceInfo {
        serde_json::from_str(include_str!("../tests/fixtures/device_info.json"))
            .unwrap_or_else(|error| panic!("valid Shelly info fixture: {error}"))
    }

    fn fixture_status() -> Map<String, Value> {
        serde_json::from_str(include_str!("../tests/fixtures/status.json"))
            .unwrap_or_else(|error| panic!("valid Shelly status fixture: {error}"))
    }

    fn fixture_config() -> Map<String, Value> {
        serde_json::from_str(include_str!("../tests/fixtures/config.json"))
            .unwrap_or_else(|error| panic!("valid Shelly config fixture: {error}"))
    }

    fn integration_id() -> IntegrationId {
        let installation_id = InstallationId::new();
        IntegrationId::from_native(&installation_id, INTEGRATION, "test")
    }

    #[test]
    fn projection_should_create_switch_light_and_cover_endpoints() {
        let target = DiscoveredShelly {
            address: "192.0.2.42"
                .parse()
                .unwrap_or_else(|error| panic!("valid fixture IP: {error}")),
            port: 80,
        };

        let snapshot = project_snapshot(
            &target,
            &integration_id(),
            fixture_info(),
            Some(fixture_status()),
            Some(fixture_config()),
        );

        let endpoint_ids: Vec<_> = snapshot.endpoints.iter().map(|item| &item.id).collect();
        assert_eq!(endpoint_ids.len(), 4);
        assert_eq!(snapshot.name, "Office cover");
        assert!(snapshot.endpoints.iter().any(|item| {
            item.capabilities
                .iter()
                .any(|capability| capability.schema() == "position.v1")
        }));
    }

    #[test]
    fn authenticated_device_should_remain_visible_without_status() {
        let target = DiscoveredShelly {
            address: IpAddr::from([192, 0, 2, 42]),
            port: 80,
        };
        let mut info = fixture_info();
        info.auth_en = true;

        let snapshot = project_snapshot(&target, &integration_id(), info, None, None);

        assert_eq!(snapshot.endpoints.len(), 1);
        assert_eq!(
            snapshot.vendor_data.get("shelly.authentication_required"),
            Some(&Value::Bool(true))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dns_sd_parser_should_keep_only_native_shelly_services() {
        let output = concat!(
            "printer._http._tcp SRV 0 0 80 printer.local.\n",
            "shellyplus2pm-aabbcc._http._tcp SRV 0 0 80 ShellyPlus2PM-AABBCC.local.\n",
            "shellyplus2pm-aabbcc._http._tcp TXT gen=2\n",
        );

        let services = parse_dns_sd_zone(output);

        assert_eq!(
            services,
            BTreeSet::from([NativeService {
                hostname: "ShellyPlus2PM-AABBCC.local".to_owned(),
                port: 80,
            }])
        );
    }
}

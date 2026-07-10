//! `HomeMagic` application services and integration ports.

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use homemagic_domain::{DeviceId, DeviceSnapshot};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::RwLock;

/// Error erased at the integration boundary.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Adapter port for discovering and refreshing device snapshots.
#[async_trait]
pub trait IntegrationScanner: Send + Sync {
    /// Returns the stable integration name used for diagnostics.
    fn integration(&self) -> &'static str;

    /// Scans the adapter's configured environment and returns current snapshots.
    ///
    /// # Errors
    ///
    /// Returns an adapter-specific error if discovery cannot complete.
    async fn scan(&self) -> Result<Vec<DeviceSnapshot>, BoxError>;
}

/// Concurrency-safe current device projection.
#[derive(Clone, Default)]
pub struct DeviceRegistry {
    devices: Arc<RwLock<BTreeMap<DeviceId, DeviceSnapshot>>>,
}

impl DeviceRegistry {
    /// Inserts or replaces device snapshots by stable identity.
    pub async fn upsert_all(&self, devices: impl IntoIterator<Item = DeviceSnapshot>) {
        let mut registry = self.devices.write().await;
        for device in devices {
            registry.insert(device.id.clone(), device);
        }
    }

    /// Returns all current snapshots in stable identifier order.
    pub async fn list(&self) -> Vec<DeviceSnapshot> {
        self.devices.read().await.values().cloned().collect()
    }

    /// Returns one current snapshot, when present.
    pub async fn get(&self, id: &DeviceId) -> Option<DeviceSnapshot> {
        self.devices.read().await.get(id).cloned()
    }
}

/// Summary of one integration refresh.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntegrationRefresh {
    /// Integration name.
    pub integration: String,
    /// Number of snapshots accepted by the registry.
    pub devices: usize,
}

/// Application service failure.
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// One integration failed its scan.
    #[error("integration `{integration}` failed: {source}")]
    Integration {
        /// Stable integration name.
        integration: String,
        /// Adapter-specific source error.
        source: BoxError,
    },
}

/// Main application facade used by RPC and future MCP transports.
#[derive(Clone)]
pub struct HomeMagicApplication {
    registry: DeviceRegistry,
    scanners: Arc<[Arc<dyn IntegrationScanner>]>,
}

impl HomeMagicApplication {
    /// Creates an application from a registry and integration scanners.
    #[must_use]
    pub fn new(
        registry: DeviceRegistry,
        scanners: impl IntoIterator<Item = Arc<dyn IntegrationScanner>>,
    ) -> Self {
        Self {
            registry,
            scanners: scanners.into_iter().collect(),
        }
    }

    /// Returns the current registry projection.
    #[must_use]
    pub const fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Refreshes every configured integration and updates current projections.
    ///
    /// # Errors
    ///
    /// Returns the first integration error. Snapshots from integrations refreshed
    /// before that error remain available.
    pub async fn refresh(&self) -> Result<Vec<IntegrationRefresh>, ApplicationError> {
        let mut summaries = Vec::with_capacity(self.scanners.len());

        for scanner in self.scanners.iter() {
            let integration = scanner.integration();
            let devices = scanner
                .scan()
                .await
                .map_err(|source| ApplicationError::Integration {
                    integration: integration.to_owned(),
                    source,
                })?;
            let count = devices.len();
            self.registry.upsert_all(devices).await;
            summaries.push(IntegrationRefresh {
                integration: integration.to_owned(),
                devices: count,
            });
        }

        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use homemagic_domain::NetworkLocation;

    use super::*;

    fn device(native_id: &str) -> DeviceSnapshot {
        DeviceSnapshot {
            id: DeviceId::from_native("test", native_id),
            native_id: native_id.to_owned(),
            integration: "test".to_owned(),
            name: native_id.to_owned(),
            manufacturer: "Test".to_owned(),
            model: "fixture".to_owned(),
            network: vec![NetworkLocation {
                host: "127.0.0.1".to_owned(),
                port: 80,
            }],
            endpoints: Vec::new(),
            observed_at: Utc::now(),
            vendor_data: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn registry_should_replace_snapshot_with_same_stable_id() {
        let registry = DeviceRegistry::default();
        let first = device("one");
        let mut updated = first.clone();
        updated.name = "updated".to_owned();

        registry.upsert_all([first]).await;
        registry.upsert_all([updated]).await;

        let devices = registry.list().await;
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "updated");
    }
}

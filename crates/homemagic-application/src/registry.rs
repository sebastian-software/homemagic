use std::collections::BTreeMap;
use std::sync::Arc;

use homemagic_domain::{DeviceId, DeviceRecord, DeviceSnapshot};
use tokio::sync::RwLock;

/// Concurrency-safe current durable device projection.
#[derive(Clone, Default)]
pub struct DeviceRegistry {
    records: Arc<RwLock<BTreeMap<DeviceId, DeviceRecord>>>,
}

impl DeviceRegistry {
    /// Replaces the complete projection loaded at startup.
    pub async fn load(&self, records: impl IntoIterator<Item = DeviceRecord>) {
        let mut registry = self.records.write().await;
        *registry = records
            .into_iter()
            .map(|record| (record.snapshot.id.clone(), record))
            .collect();
    }

    /// Inserts or replaces durable records by stable identity.
    pub async fn upsert_all(&self, records: impl IntoIterator<Item = DeviceRecord>) {
        let mut registry = self.records.write().await;
        for record in records {
            registry.insert(record.snapshot.id.clone(), record);
        }
    }

    /// Returns adapter-compatible snapshots in stable identifier order.
    pub async fn list(&self) -> Vec<DeviceSnapshot> {
        self.records
            .read()
            .await
            .values()
            .map(|record| record.snapshot.clone())
            .collect()
    }

    /// Returns durable records in stable identifier order.
    pub async fn records(&self) -> Vec<DeviceRecord> {
        self.records.read().await.values().cloned().collect()
    }

    /// Returns one adapter-compatible snapshot, when present.
    pub async fn get(&self, id: &DeviceId) -> Option<DeviceSnapshot> {
        self.records
            .read()
            .await
            .get(id)
            .map(|record| record.snapshot.clone())
    }

    /// Returns one durable record, when present.
    pub async fn get_record(&self, id: &DeviceId) -> Option<DeviceRecord> {
        self.records.read().await.get(id).cloned()
    }
}

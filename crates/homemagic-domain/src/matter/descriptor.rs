use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::MatterFabricId;

/// Maximum endpoints accepted in one node descriptor.
pub const MAX_MATTER_ENDPOINTS_PER_NODE: usize = 256;
/// Maximum device types accepted on one endpoint.
pub const MAX_MATTER_DEVICE_TYPES_PER_ENDPOINT: usize = 16;
/// Maximum server or client clusters accepted on one endpoint.
pub const MAX_MATTER_CLUSTERS_PER_ENDPOINT: usize = 128;
/// Maximum accepted command identifiers on one cluster.
pub const MAX_MATTER_COMMANDS_PER_CLUSTER: usize = 128;
/// Maximum accepted attribute identifiers on one cluster.
pub const MAX_MATTER_ATTRIBUTES_PER_CLUSTER: usize = 256;

/// Fabric-scoped operational Matter node identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MatterNodeId(u64);

impl MatterNodeId {
    /// Creates a non-zero operational node identifier.
    ///
    /// # Errors
    ///
    /// Returns [`MatterDescriptorError::ZeroNodeId`] for zero.
    pub const fn new(value: u64) -> Result<Self, MatterDescriptorError> {
        if value == 0 {
            Err(MatterDescriptorError::ZeroNodeId)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the protocol node identifier.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for MatterNodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Positive monotonic Matter descriptor revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MatterDescriptorRevision(u64);

impl MatterDescriptorRevision {
    /// Creates a positive descriptor revision.
    ///
    /// # Errors
    ///
    /// Returns [`MatterDescriptorError::ZeroRevision`] for zero.
    pub const fn new(value: u64) -> Result<Self, MatterDescriptorError> {
        if value == 0 {
            Err(MatterDescriptorError::ZeroRevision)
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

impl<'de> Deserialize<'de> for MatterDescriptorRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Matter endpoint number within one node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MatterEndpointNumber(u16);

impl MatterEndpointNumber {
    /// Creates an endpoint number. Endpoint zero is the valid root endpoint.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the protocol endpoint number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// One endpoint device-type declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MatterDeviceType {
    /// Numeric Matter device-type identifier.
    pub id: u32,
    /// Positive device-type revision.
    pub revision: u16,
}

impl<'de> Deserialize<'de> for MatterDeviceType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Data {
            id: u32,
            revision: u16,
        }

        let data = Data::deserialize(deserializer)?;
        Self::new(data.id, data.revision).map_err(D::Error::custom)
    }
}

impl MatterDeviceType {
    /// Creates a device-type declaration with a positive revision.
    ///
    /// # Errors
    ///
    /// Returns [`MatterDescriptorError::ZeroRevision`] for revision zero.
    pub const fn new(id: u32, revision: u16) -> Result<Self, MatterDescriptorError> {
        if revision == 0 {
            Err(MatterDescriptorError::ZeroRevision)
        } else {
            Ok(Self { id, revision })
        }
    }
}

/// One server or client cluster declaration independent from an SDK.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterClusterDescriptor {
    id: u32,
    revision: u16,
    feature_map: u32,
    accepted_commands: Vec<u32>,
    attributes: Vec<u32>,
}

impl MatterClusterDescriptor {
    /// Creates a bounded cluster declaration.
    ///
    /// # Errors
    ///
    /// Rejects revision zero, excessive command identifiers, or duplicates.
    pub fn new(
        id: u32,
        revision: u16,
        feature_map: u32,
        accepted_commands: Vec<u32>,
    ) -> Result<Self, MatterDescriptorError> {
        Self::with_attributes(id, revision, feature_map, accepted_commands, Vec::new())
    }

    /// Creates a bounded cluster declaration including available attributes.
    ///
    /// # Errors
    ///
    /// Rejects revision zero, excessive identifiers, or duplicates.
    pub fn with_attributes(
        id: u32,
        revision: u16,
        feature_map: u32,
        accepted_commands: Vec<u32>,
        attributes: Vec<u32>,
    ) -> Result<Self, MatterDescriptorError> {
        if revision == 0 {
            return Err(MatterDescriptorError::ZeroRevision);
        }
        ensure_bounded_unique(
            &accepted_commands,
            MAX_MATTER_COMMANDS_PER_CLUSTER,
            MatterDescriptorCollection::Commands,
        )?;
        ensure_bounded_unique(
            &attributes,
            MAX_MATTER_ATTRIBUTES_PER_CLUSTER,
            MatterDescriptorCollection::Attributes,
        )?;
        Ok(Self {
            id,
            revision,
            feature_map,
            accepted_commands,
            attributes,
        })
    }

    /// Returns the numeric cluster identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// Returns the cluster revision.
    #[must_use]
    pub const fn revision(&self) -> u16 {
        self.revision
    }

    /// Returns the reported feature map.
    #[must_use]
    pub const fn feature_map(&self) -> u32 {
        self.feature_map
    }

    /// Returns bounded accepted command identifiers.
    #[must_use]
    pub fn accepted_commands(&self) -> &[u32] {
        &self.accepted_commands
    }

    /// Returns bounded available attribute identifiers.
    #[must_use]
    pub fn attributes(&self) -> &[u32] {
        &self.attributes
    }
}

#[derive(Deserialize)]
struct MatterClusterDescriptorData {
    id: u32,
    revision: u16,
    feature_map: u32,
    accepted_commands: Vec<u32>,
    #[serde(default)]
    attributes: Vec<u32>,
}

impl<'de> Deserialize<'de> for MatterClusterDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterClusterDescriptorData::deserialize(deserializer)?;
        Self::with_attributes(
            data.id,
            data.revision,
            data.feature_map,
            data.accepted_commands,
            data.attributes,
        )
        .map_err(D::Error::custom)
    }
}

/// Bounded Matter endpoint descriptor used for projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterEndpointDescriptor {
    number: MatterEndpointNumber,
    device_types: Vec<MatterDeviceType>,
    server_clusters: Vec<MatterClusterDescriptor>,
    client_clusters: Vec<MatterClusterDescriptor>,
}

impl MatterEndpointDescriptor {
    /// Creates a bounded endpoint descriptor with unique identifiers.
    ///
    /// # Errors
    ///
    /// Rejects excessive or duplicate device types and clusters.
    pub fn new(
        number: MatterEndpointNumber,
        device_types: Vec<MatterDeviceType>,
        server_clusters: Vec<MatterClusterDescriptor>,
        client_clusters: Vec<MatterClusterDescriptor>,
    ) -> Result<Self, MatterDescriptorError> {
        ensure_bounded_unique_by(
            &device_types,
            MAX_MATTER_DEVICE_TYPES_PER_ENDPOINT,
            MatterDescriptorCollection::DeviceTypes,
            |item| item.id,
        )?;
        ensure_bounded_unique_by(
            &server_clusters,
            MAX_MATTER_CLUSTERS_PER_ENDPOINT,
            MatterDescriptorCollection::ServerClusters,
            MatterClusterDescriptor::id,
        )?;
        ensure_bounded_unique_by(
            &client_clusters,
            MAX_MATTER_CLUSTERS_PER_ENDPOINT,
            MatterDescriptorCollection::ClientClusters,
            MatterClusterDescriptor::id,
        )?;
        Ok(Self {
            number,
            device_types,
            server_clusters,
            client_clusters,
        })
    }

    /// Returns the endpoint number.
    #[must_use]
    pub const fn number(&self) -> MatterEndpointNumber {
        self.number
    }

    /// Returns declared device types.
    #[must_use]
    pub fn device_types(&self) -> &[MatterDeviceType] {
        &self.device_types
    }

    /// Returns server clusters.
    #[must_use]
    pub fn server_clusters(&self) -> &[MatterClusterDescriptor] {
        &self.server_clusters
    }

    /// Returns client clusters.
    #[must_use]
    pub fn client_clusters(&self) -> &[MatterClusterDescriptor] {
        &self.client_clusters
    }
}

#[derive(Deserialize)]
struct MatterEndpointDescriptorData {
    number: MatterEndpointNumber,
    device_types: Vec<MatterDeviceType>,
    server_clusters: Vec<MatterClusterDescriptor>,
    client_clusters: Vec<MatterClusterDescriptor>,
}

impl<'de> Deserialize<'de> for MatterEndpointDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterEndpointDescriptorData::deserialize(deserializer)?;
        Self::new(
            data.number,
            data.device_types,
            data.server_clusters,
            data.client_clusters,
        )
        .map_err(D::Error::custom)
    }
}

/// Bounded node descriptor tied to one `HomeMagic` fabric.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatterNodeDescriptor {
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    endpoints: Vec<MatterEndpointDescriptor>,
    descriptor_revision: MatterDescriptorRevision,
}

impl MatterNodeDescriptor {
    /// Creates a node descriptor with unique bounded endpoints.
    ///
    /// # Errors
    ///
    /// Rejects revision zero, excessive endpoints, or duplicate endpoint numbers.
    pub fn new(
        fabric_id: MatterFabricId,
        node_id: MatterNodeId,
        endpoints: Vec<MatterEndpointDescriptor>,
        descriptor_revision: MatterDescriptorRevision,
    ) -> Result<Self, MatterDescriptorError> {
        ensure_bounded_unique_by(
            &endpoints,
            MAX_MATTER_ENDPOINTS_PER_NODE,
            MatterDescriptorCollection::Endpoints,
            MatterEndpointDescriptor::number,
        )?;
        Ok(Self {
            fabric_id,
            node_id,
            endpoints,
            descriptor_revision,
        })
    }

    /// Returns the owning `HomeMagic` fabric identity.
    #[must_use]
    pub fn fabric_id(&self) -> &MatterFabricId {
        &self.fabric_id
    }

    /// Returns the fabric-scoped operational node ID.
    #[must_use]
    pub const fn node_id(&self) -> MatterNodeId {
        self.node_id
    }

    /// Returns endpoint descriptors in adapter-provided order.
    #[must_use]
    pub fn endpoints(&self) -> &[MatterEndpointDescriptor] {
        &self.endpoints
    }

    /// Returns the monotonic descriptor revision.
    #[must_use]
    pub const fn descriptor_revision(&self) -> MatterDescriptorRevision {
        self.descriptor_revision
    }
}

#[derive(Deserialize)]
struct MatterNodeDescriptorData {
    fabric_id: MatterFabricId,
    node_id: MatterNodeId,
    endpoints: Vec<MatterEndpointDescriptor>,
    descriptor_revision: MatterDescriptorRevision,
}

impl<'de> Deserialize<'de> for MatterNodeDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MatterNodeDescriptorData::deserialize(deserializer)?;
        Self::new(
            data.fabric_id,
            data.node_id,
            data.endpoints,
            data.descriptor_revision,
        )
        .map_err(D::Error::custom)
    }
}

/// Descriptor collection involved in a validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatterDescriptorCollection {
    /// Node endpoint list.
    Endpoints,
    /// Endpoint device-type list.
    DeviceTypes,
    /// Endpoint server-cluster list.
    ServerClusters,
    /// Endpoint client-cluster list.
    ClientClusters,
    /// Cluster accepted-command list.
    Commands,
    /// Cluster available-attribute list.
    Attributes,
}

/// Invalid bounded Matter descriptor data.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterDescriptorError {
    /// Operational node IDs cannot be zero.
    #[error("Matter operational node ID must be non-zero")]
    ZeroNodeId,
    /// Protocol and descriptor revisions start at one.
    #[error("Matter descriptor revision must be non-zero")]
    ZeroRevision,
    /// A descriptor collection exceeded its fixed bound.
    #[error("Matter {collection:?} collection exceeds {maximum} items")]
    TooManyItems {
        /// Collection that exceeded its bound.
        collection: MatterDescriptorCollection,
        /// Maximum accepted items.
        maximum: usize,
    },
    /// A descriptor collection contained a duplicate stable key.
    #[error("Matter {collection:?} collection contains a duplicate identifier")]
    DuplicateIdentifier {
        /// Collection containing the duplicate.
        collection: MatterDescriptorCollection,
    },
}

fn ensure_bounded_unique<T>(
    items: &[T],
    maximum: usize,
    collection: MatterDescriptorCollection,
) -> Result<(), MatterDescriptorError>
where
    T: Copy + Ord,
{
    ensure_bounded_unique_by(items, maximum, collection, |item| *item)
}

fn ensure_bounded_unique_by<T, K>(
    items: &[T],
    maximum: usize,
    collection: MatterDescriptorCollection,
    key: impl Fn(&T) -> K,
) -> Result<(), MatterDescriptorError>
where
    K: Ord,
{
    if items.len() > maximum {
        return Err(MatterDescriptorError::TooManyItems {
            collection,
            maximum,
        });
    }
    let mut seen = BTreeSet::new();
    if items.iter().any(|item| !seen.insert(key(item))) {
        return Err(MatterDescriptorError::DuplicateIdentifier { collection });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    fn cluster(id: u32) -> Result<MatterClusterDescriptor, MatterDescriptorError> {
        MatterClusterDescriptor::new(id, 1, 0, Vec::new())
    }

    #[test]
    fn node_id_should_reject_zero() {
        assert_eq!(MatterNodeId::new(0), Err(MatterDescriptorError::ZeroNodeId));
    }

    #[test]
    fn endpoint_should_reject_duplicate_server_clusters() -> Result<(), MatterDescriptorError> {
        let result = MatterEndpointDescriptor::new(
            MatterEndpointNumber::new(1),
            Vec::new(),
            vec![cluster(6)?, cluster(6)?],
            Vec::new(),
        );

        assert_eq!(
            result,
            Err(MatterDescriptorError::DuplicateIdentifier {
                collection: MatterDescriptorCollection::ServerClusters,
            })
        );
        Ok(())
    }

    #[test]
    fn node_descriptor_should_round_trip_through_json() -> Result<(), Box<dyn Error>> {
        let endpoint = MatterEndpointDescriptor::new(
            MatterEndpointNumber::new(1),
            vec![MatterDeviceType::new(0x0100, 1)?],
            vec![cluster(6)?],
            Vec::new(),
        )?;
        let descriptor = MatterNodeDescriptor::new(
            MatterFabricId::new(),
            MatterNodeId::new(42)?,
            vec![endpoint],
            MatterDescriptorRevision::new(1)?,
        )?;

        let encoded = serde_json::to_string(&descriptor)?;
        let decoded = serde_json::from_str(&encoded)?;

        assert_eq!(descriptor, decoded);
        Ok(())
    }

    #[test]
    fn deserialization_should_reapply_command_bounds() {
        let commands = (0..=MAX_MATTER_COMMANDS_PER_CLUSTER)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let json =
            format!(r#"{{"id":6,"revision":1,"feature_map":0,"accepted_commands":[{commands}]}}"#);

        let result = serde_json::from_str::<MatterClusterDescriptor>(&json);

        assert!(result.is_err(), "oversized command list should be rejected");
    }

    #[test]
    fn cluster_should_reject_duplicate_and_oversized_attribute_lists() {
        let duplicate = MatterClusterDescriptor::with_attributes(6, 1, 0, Vec::new(), vec![0, 0]);
        let oversized = MatterClusterDescriptor::with_attributes(
            6,
            1,
            0,
            Vec::new(),
            (0..=MAX_MATTER_ATTRIBUTES_PER_CLUSTER)
                .filter_map(|value| u32::try_from(value).ok())
                .collect(),
        );

        assert_eq!(
            duplicate,
            Err(MatterDescriptorError::DuplicateIdentifier {
                collection: MatterDescriptorCollection::Attributes,
            })
        );
        assert_eq!(
            oversized,
            Err(MatterDescriptorError::TooManyItems {
                collection: MatterDescriptorCollection::Attributes,
                maximum: MAX_MATTER_ATTRIBUTES_PER_CLUSTER,
            })
        );
    }
}

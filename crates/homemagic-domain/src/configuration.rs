use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{InstallationId, IntegrationId, SecretRef, SpaceId};

/// Durable installation configuration and display metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Installation {
    /// Stable installation identity.
    pub id: InstallationId,
    /// Mutable human-readable installation name.
    pub name: String,
    /// Time the installation was created.
    pub created_at: DateTime<Utc>,
}

/// Durable configuration for one adapter instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IntegrationInstance {
    /// Stable integration-instance identity.
    pub id: IntegrationId,
    /// Installation that owns this integration.
    pub installation_id: InstallationId,
    /// Stable adapter name, such as `shelly`.
    pub adapter: String,
    /// Immutable key unique within the installation and adapter.
    pub instance_key: String,
    /// Mutable display name.
    pub name: String,
    /// Opaque reference to shared integration credentials, when configured.
    #[serde(default)]
    pub credential_ref: Option<SecretRef>,
}

/// Durable semantic space independent of its mutable name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Space {
    /// Stable space identity.
    pub id: SpaceId,
    /// Installation that owns this space.
    pub installation_id: InstallationId,
    /// Optional parent in the semantic space tree.
    pub parent_id: Option<SpaceId>,
    /// Mutable display name.
    pub name: String,
    /// Additional mutable names used for intent resolution.
    pub aliases: BTreeSet<String>,
}

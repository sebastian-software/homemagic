use std::collections::BTreeMap;

use homemagic_domain::{
    MatterAttributePath, MatterAttributeValue, MatterClusterDescriptor, MatterDescriptorError,
    MatterDescriptorRevision, MatterDeviceType, MatterEndpointDescriptor, MatterEndpointNumber,
    MatterFabricId, MatterNodeDescriptor, MatterNodeId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Matter On/Off cluster identifier.
pub const ON_OFF_CLUSTER_ID: u32 = 0x0006;
/// `OnOff` attribute identifier.
pub const ON_OFF_ATTRIBUTE_ID: u32 = 0x0000;
/// Matter Door Lock cluster identifier.
pub const DOOR_LOCK_CLUSTER_ID: u32 = 0x0101;
/// `LockState` attribute identifier.
pub const DOOR_LOCK_STATE_ATTRIBUTE_ID: u32 = 0x0000;

/// Versioned built-in simulator fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulatorFixture {
    /// On/Off light fixture version one.
    LightV1,
    /// Door Lock fixture version one.
    DoorLockV1,
}

impl SimulatorFixture {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::LightV1 => "light-v1",
            Self::DoorLockV1 => "door-lock-v1",
        }
    }

    pub(crate) const fn node_id(self) -> u64 {
        match self {
            Self::LightV1 => 0x1001,
            Self::DoorLockV1 => 0x2001,
        }
    }

    pub(crate) fn materialize(
        self,
        fabric_id: MatterFabricId,
    ) -> Result<MaterializedFixture, SimulatorFixtureError> {
        let node_id = MatterNodeId::new(self.node_id())?;
        let endpoint = MatterEndpointNumber::new(1);
        let (device_type, cluster_id, accepted_commands, initial_value) = match self {
            Self::LightV1 => (
                0x0100,
                ON_OFF_CLUSTER_ID,
                vec![0x0000, 0x0001],
                MatterAttributeValue::Boolean(false),
            ),
            Self::DoorLockV1 => (
                0x000A,
                DOOR_LOCK_CLUSTER_ID,
                vec![0x0000, 0x0001],
                MatterAttributeValue::Unsigned(1),
            ),
        };
        let descriptor = MatterNodeDescriptor::new(
            fabric_id,
            node_id,
            vec![MatterEndpointDescriptor::new(
                endpoint,
                vec![MatterDeviceType::new(device_type, 1)?],
                vec![MatterClusterDescriptor::with_attributes(
                    cluster_id,
                    1,
                    0,
                    accepted_commands,
                    vec![0],
                )?],
                Vec::new(),
            )?],
            MatterDescriptorRevision::new(1)?,
        )?;
        let path = MatterAttributePath {
            node_id,
            endpoint,
            cluster_id,
            attribute_id: 0,
        };
        Ok(MaterializedFixture {
            fixture: self,
            descriptor,
            attributes: BTreeMap::from([(path, initial_value)]),
        })
    }
}

pub(crate) struct MaterializedFixture {
    pub fixture: SimulatorFixture,
    pub descriptor: MatterNodeDescriptor,
    pub attributes: BTreeMap<MatterAttributePath, MatterAttributeValue>,
}

/// Built-in fixture construction failed its domain contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SimulatorFixtureError {
    /// A fixed fixture violated the Matter descriptor contract.
    #[error("built-in simulator fixture violates its descriptor contract")]
    InvalidDescriptor,
}

impl From<MatterDescriptorError> for SimulatorFixtureError {
    fn from(_value: MatterDescriptorError) -> Self {
        Self::InvalidDescriptor
    }
}

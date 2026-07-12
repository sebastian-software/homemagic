use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{ensure, Context, Result};
use matc::{
    clusters::{
        codec::{descriptor_cluster, on_off},
        defs::{CLUSTER_ID_ON_OFF, CLUSTER_ON_OFF_ATTR_ID_ONOFF},
    },
    devman::{DeviceManager, ManagerConfig},
    tlv::{TlvItemEnc, TlvItemValueEnc},
};
use serde_json::json;

const FABRIC_ID: u64 = 0x484f_4d45;
const CONTROLLER_ID: u64 = 0x1001;
const NODE_ID: u64 = 0x1002;
const DEVICE_NAME: &str = "independent-rs-matter-light";
const PASSCODE: u32 = 20_202_021;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let data_dir = args.next().context("missing controller data directory")?;
    let device_address = args.next().unwrap_or_else(|| "[::1]:5540".to_owned());
    ensure!(args.next().is_none(), "unexpected argument");

    let manager = DeviceManager::create(
        &data_dir,
        ManagerConfig {
            fabric_id: FABRIC_ID,
            controller_id: CONTROLLER_ID,
            local_address: "[::]:0".to_owned(),
        },
    )
    .await
    .context("create fabric and controller")?;

    let phase = Arc::new(Mutex::new("commission"));
    let lifecycle = tokio::time::timeout(
        Duration::from_secs(45),
        run_lifecycle(manager, &data_dir, &device_address, Arc::clone(&phase)),
    )
    .await;

    let failed_phase = *phase.lock().expect("phase lock poisoned");
    let (failure, outcomes) = match lifecycle {
        Ok(Ok(())) => (
            None,
            json!({
                "fabric_create": "pass",
                "commission": "pass",
                "inventory": "pass",
                "read": "pass",
                "invoke": "pass",
                "subscribe": "pass",
                "controller_restart": "pass",
                "remote_remove_fabric": "pass",
                "local_remove": "pass"
            }),
        ),
        Ok(Err(_)) => (
            Some("candidate_error"),
            incomplete_outcomes(failed_phase, "fail"),
        ),
        Err(_) => (
            Some("candidate_timeout"),
            incomplete_outcomes(failed_phase, "timeout"),
        ),
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "homemagic.matter.independent-reference.v1",
            "candidate": "rust-matc",
            "candidate_revision": "c829d2a1b570b2f2433607a3f4731074b73fb367",
            "reference": "rs-matter-onoff-light",
            "reference_revision": "42d3c2211239f5f388ac7f7449c82bb3347912f5",
            "transport": "on_network_ipv6",
            "failure": failure,
            "failed_phase": failure.map(|_| failed_phase),
            "outcomes": outcomes
        }))?
    );

    Ok(())
}

fn incomplete_outcomes(failed_phase: &str, failure: &str) -> serde_json::Value {
    let mut outcomes = serde_json::Map::new();
    outcomes.insert("fabric_create".to_owned(), json!("pass"));
    let mut after_failure = false;
    for phase in [
        "commission",
        "inventory",
        "read",
        "subscribe",
        "invoke",
        "controller_restart",
        "remote_remove_fabric",
        "local_remove",
    ] {
        let outcome = if phase == failed_phase {
            after_failure = true;
            failure
        } else if after_failure {
            "not_run"
        } else {
            "pass"
        };
        outcomes.insert(phase.to_owned(), json!(outcome));
    }
    serde_json::Value::Object(outcomes)
}

async fn run_lifecycle(
    manager: DeviceManager,
    data_dir: &str,
    device_address: &str,
    phase: Arc<Mutex<&'static str>>,
) -> Result<()> {
    set_phase(&phase, "commission");
    let connection = manager
        .commission(device_address, PASSCODE, NODE_ID, DEVICE_NAME)
        .await
        .context("on-network commission independent device")?;

    set_phase(&phase, "inventory");
    let inventory = manager.list_devices().context("list inventory")?;
    ensure!(inventory.len() == 1 && inventory[0].node_id == NODE_ID);
    let endpoints = descriptor_cluster::read_parts_list(&connection, 0)
        .await
        .context("read descriptor parts list")?;
    ensure!(endpoints.contains(&1), "endpoint 1 absent from descriptor");

    set_phase(&phase, "read");
    let initial_on = on_off::read_on_off(&connection, 1)
        .await
        .context("read OnOff before invoke")?;
    ensure!(!initial_on, "independent fixture should start off");

    set_phase(&phase, "subscribe");
    let mut subscription = connection
        .subscribe_attrs(
            Some(1),
            Some(CLUSTER_ID_ON_OFF),
            Some(CLUSTER_ON_OFF_ATTR_ID_ONOFF),
            false,
        )
        .await
        .context("subscribe to OnOff")?;
    ensure!(!subscription.priming_attribute_reports.is_empty());

    set_phase(&phase, "invoke");
    on_off::on(&connection, 1).await.context("invoke On")?;
    tokio::time::timeout(Duration::from_secs(10), subscription.next())
        .await
        .context("subscription update timeout")?
        .context("subscription closed before update")?;
    ensure!(
        on_off::read_on_off(&connection, 1)
            .await
            .context("read OnOff after On")?,
        "On invoke did not converge"
    );

    drop(subscription);
    drop(connection);
    drop(manager);
    tokio::time::sleep(Duration::from_millis(250)).await;

    set_phase(&phase, "controller_restart");
    let manager = DeviceManager::load(&data_dir)
        .await
        .context("reload controller state")?;
    ensure!(manager.list_devices()?.len() == 1);
    let connection = manager
        .connect_by_name(DEVICE_NAME)
        .await
        .context("CASE reconnect after controller restart")?;
    ensure!(on_off::read_on_off(&connection, 1)
        .await
        .context("read after controller restart")?);
    on_off::off(&connection, 1).await.context("invoke Off")?;
    ensure!(!on_off::read_on_off(&connection, 1).await?);

    set_phase(&phase, "remote_remove_fabric");
    let remove_fabric = TlvItemEnc {
        tag: 0,
        value: TlvItemValueEnc::UInt8(1),
    }
    .encode()
    .context("encode RemoveFabric")?;
    connection
        .invoke_request(0, 0x003e, 0x000a, &remove_fabric)
        .await
        .context("invoke remote RemoveFabric")?;
    set_phase(&phase, "local_remove");
    manager
        .remove_device(NODE_ID)
        .context("remove local inventory record")?;
    ensure!(manager.list_devices()?.is_empty());

    Ok(())
}

fn set_phase(progress: &Mutex<&'static str>, phase: &'static str) {
    *progress.lock().expect("phase lock poisoned") = phase;
}

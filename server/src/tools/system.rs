//! System information tool (sysinfo).

use super::ToolOutput;
use crate::platform;
use crate::state::AppState;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Disks, Networks, System};

pub async fn info(_state: Arc<AppState>, _args: serde_json::Value) -> Result<ToolOutput, String> {
    let mut sys = System::new_all();
    tokio::time::sleep(Duration::from_millis(200)).await;
    sys.refresh_all();

    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let cpu_freq = sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0);
    let load = System::load_average();

    let disks = Disks::new_with_refreshed_list();
    let disk_rows: Vec<serde_json::Value> = disks
        .list()
        .iter()
        .map(|d| {
            json!({
                "name": d.name().to_string_lossy(),
                "mount_point": d.mount_point().to_string_lossy(),
                "file_system": d.file_system().to_string_lossy(),
                "total_bytes": d.total_space(),
                "available_bytes": d.available_space(),
                "removable": d.is_removable(),
            })
        })
        .collect();

    let mut networks = Networks::new_with_refreshed_list();
    tokio::time::sleep(Duration::from_millis(150)).await;
    networks.refresh(true);
    let net_rows: Vec<serde_json::Value> = networks
        .list()
        .iter()
        .map(|(name, data)| {
            let addrs: Vec<String> = data
                .ip_networks()
                .iter()
                .map(|n| format!("{}/{}", n.addr, n.prefix))
                .collect();
            json!({
                "interface": name,
                "mac": data.mac_address().to_string(),
                "addresses": addrs,
                "received_bytes": data.received(),
                "transmitted_bytes": data.transmitted(),
            })
        })
        .collect();

    let body = json!({
        "os": {
            "name": System::name(),
            "version": System::os_version(),
            "long_version": System::long_os_version(),
            "kernel": System::kernel_version(),
            "hostname": System::host_name(),
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        },
        "uptime_secs": System::uptime(),
        "load_average": {"one": load.one, "five": load.five, "fifteen": load.fifteen},
        "cpu": {
            "brand": cpu_brand,
            "physical_cores": System::physical_core_count(),
            "logical_cores": sys.cpus().len(),
            "frequency_mhz": cpu_freq,
            "usage_percent": (sys.global_cpu_usage() * 10.0).round() / 10.0,
        },
        "memory": {
            "total_bytes": sys.total_memory(),
            "used_bytes": sys.used_memory(),
            "available_bytes": sys.available_memory(),
            "swap_total_bytes": sys.total_swap(),
            "swap_used_bytes": sys.used_swap(),
        },
        "disks": disk_rows,
        "networks": net_rows,
        "session": platform::session_info(),
        "displays": platform::displays(),
        "backends": platform::backends_report(),
    });
    Ok(ToolOutput::json(body))
}

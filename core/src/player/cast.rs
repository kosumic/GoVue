//! Cast discovery presentation. Device advertisements are untrusted LAN input;
//! no discovery data or session addresses are persisted.
use crate::Result;
use serde_json::{Value, json};
use std::net::IpAddr;

pub(super) fn devices(command: &Value) -> Result<Value> {
    let raw = crate::string(command, "raw")?;
    if raw.len() > 131_072 {
        return Err("Too many cast receivers.".into());
    }
    let data: Value = serde_json::from_str(raw).map_err(|_| "Unreadable cast receivers.")?;
    let mut devices = Vec::new();
    for device in data["devices"].as_array().into_iter().flatten().take(100) {
        let Some(host) = device["host"].as_str().filter(|s| s.len() <= 160) else {
            continue;
        };
        let Some(port) = device["port"].as_u64().filter(|p| (1..=65535).contains(p)) else {
            continue;
        };
        let Ok(address) = host.split('%').next().unwrap_or("").parse::<IpAddr>() else {
            continue;
        };
        let private = match address {
            IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
            IpAddr::V6(ip) => {
                (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80
            }
        };
        if !private
            || host
                .bytes()
                .any(|b| !(b.is_ascii_alphanumeric() || b":.%_-".contains(&b)))
        {
            continue;
        }
        let id = format!("{host}:{port}");
        if devices.iter().any(|d: &Value| d["id"] == id) {
            continue;
        }
        let name = crate::clean(device["name"].as_str().unwrap_or("Cast"))
            .chars()
            .take(200)
            .collect::<String>();
        devices.push(json!({"id":id,"name":name,"host":host,"port":port}));
    }
    devices.sort_by_cached_key(|d| d["name"].as_str().unwrap_or("").to_lowercase());
    Ok(
        json!({"available":data["available"]==true,"devices":devices,"error":if data["error"].is_string(){Some("Device discovery could not start. Check local-network access.")}else{None},"transportAvailable":crate::cast::available()}),
    )
}

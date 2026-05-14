use std::net::IpAddr;

use crate::host::{ip_is_loopback, target_host_is_loopback};
use crate::types::{Route, RouteMode};

pub(super) fn target_authority_host(host: &str) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{host}]"),
        _ => host.to_string(),
    }
}

pub(super) fn route_allowed_for_remote_client(route: &Route, remote_ip: IpAddr) -> bool {
    if route.target_host.parse::<IpAddr>().is_err() {
        return false;
    }
    // LAN clients may reach only Jig-supervised process routes on loopback
    // targets. Alias routes remain loopback-client only even when they point at
    // loopback because Jig does not own or supervise those target processes.
    ip_is_loopback(remote_ip)
        || (route.mode == RouteMode::Process && target_host_is_loopback(&route.target_host))
}

pub(super) fn route_targets_active_proxy_listener(
    route: &Route,
    connection_local_ip: IpAddr,
    lan_ip: Option<IpAddr>,
    proxy_ports: &[u16],
) -> bool {
    if !proxy_ports.contains(&route.target_port) {
        return false;
    }
    let Ok(target_ip) = route.target_host.parse::<IpAddr>() else {
        return false;
    };
    ip_is_loopback(target_ip)
        || target_ip.is_unspecified()
        || target_ip == connection_local_ip
        || lan_ip.is_some_and(|lan_ip| target_ip == lan_ip)
}

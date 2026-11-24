//! LAN worlds: vanilla's `LanServerDetection`, done by the relay because a
//! page can't hear multicast. Open-to-LAN games announce
//! `[MOTD]..[/MOTD][AD]port[/AD]` to 224.0.2.60:4445 every 1.5 seconds; the
//! page polls `/lan` for the ones heard lately.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::{AppState, guard};

const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
const PORT: u16 = 4445;
/// A world not heard from for this long is gone.
const EXPIRY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Lan {
    /// Address to (MOTD, last heard).
    servers: Mutex<HashMap<String, (String, Instant)>>,
}

/// `LanServerPinger.parseMotd` and `parseAddress`.
fn parse(message: &str) -> (String, Option<String>) {
    let between = |open: &str, close: &str| {
        let start = message.find(open)? + open.len();
        let end = message[start..].find(close)? + start;
        Some(message[start..end].to_owned())
    };
    (between("[MOTD]", "[/MOTD]").unwrap_or_else(|| "missing no".into()), between("[AD]", "[/AD]"))
}

pub fn start() -> Arc<Lan> {
    let lan = Arc::new(Lan::default());
    let socket = (|| -> std::io::Result<std::net::UdpSocket> {
        let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into())?;
        socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
        Ok(socket.into())
    })();
    let socket = match socket {
        Ok(socket) => socket,
        Err(e) => {
            tracing::warn!("Can't listen for LAN worlds: {e}");
            return lan;
        }
    };
    let found = Arc::clone(&lan);
    std::thread::Builder::new()
        .name("lan".into())
        .spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                let message = String::from_utf8_lossy(&buf[..n]);
                let (motd, port) = parse(&message);
                let Some(port) = port.and_then(|p| p.trim().parse::<u16>().ok()) else { continue };
                let address = format!("{}:{port}", from.ip());
                found.servers.lock().unwrap().insert(address, (motd, Instant::now()));
            }
        })
        .expect("start the LAN listener");
    lan
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `GET /lan`: `[{"motd": .., "address": ..}]`, for this machine's page only.
pub async fn list(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let Some(lan) = &state.lan else { return StatusCode::NOT_FOUND.into_response() };
    if !peer.ip().is_loopback() || guard::cross_site(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut servers = lan.servers.lock().unwrap();
    servers.retain(|_, (_, heard)| heard.elapsed() < EXPIRY);
    let mut entries: Vec<_> = servers.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let body = entries
        .iter()
        .map(|(address, (motd, _))| format!("{{\"motd\":{},\"address\":{}}}", json_string(motd), json_string(address)))
        .collect::<Vec<_>>()
        .join(",");
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], format!("[{body}]"))
        .into_response()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_vanilla_announcements() {
        let (motd, port) = super::parse("[MOTD]Steve - World[/MOTD][AD]51234[/AD]");
        assert_eq!((motd.as_str(), port.as_deref()), ("Steve - World", Some("51234")));
        assert_eq!(super::parse("junk").0, "missing no");
    }
}

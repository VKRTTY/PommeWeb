//! Explicitly local singleplayer. Each WebSocket owns one integrated
//! server and its save lock; this never exposes a public TCP game port.
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use pomme_singleplayer::{Difficulty, GameType, LaunchOptions, ServerTransport};

use crate::{AppState, connect, guard};

pub struct Worlds {
    root: PathBuf,
    /// Worlds open now, with the client address each belongs to.
    active: Mutex<Vec<(String, IpAddr)>>,
    /// How many worlds may be open at once: 1 for a loopback relay, the
    /// `--hosted-worlds` count for a public one.
    capacity: usize,
    /// Anyone may open a world, not only this machine (`--hosted-worlds`).
    public: bool,
}

impl Worlds {
    pub fn new(root: PathBuf, capacity: usize, public: bool) -> Self {
        Self { root, active: Mutex::new(Vec::new()), capacity: capacity.max(1), public }
    }

    /// A session for world `id`, unless it's open already, the relay is full,
    /// or (on a public relay) `client` already has a world open.
    fn reserve(self: &Arc<Self>, id: String, client: IpAddr) -> Option<Session> {
        let mut active = self.active.lock().unwrap();
        if active.len() >= self.capacity
            || active.iter().any(|(open, ip)| *open == id || (self.public && *ip == client))
        {
            return None;
        }
        active.push((id.clone(), client));
        Some(Session(Arc::clone(self), id))
    }

    fn is_open(&self, id: &str) -> bool {
        self.active.lock().unwrap().iter().any(|(open, _)| open == id)
    }

    fn allows(&self, peer: IpAddr) -> bool {
        self.public || peer.is_loopback()
    }
}

struct Session(Arc<Worlds>, String);
impl Drop for Session {
    fn drop(&mut self) {
        self.0.active.lock().unwrap().retain(|(open, _)| *open != self.1);
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub async fn status(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let status = match &state.worlds {
        None => StatusCode::NOT_FOUND,
        Some(worlds) if !worlds.allows(peer.ip()) || guard::cross_site(&headers) || !valid_id(&id) => {
            StatusCode::FORBIDDEN
        }
        Some(worlds) if worlds.is_open(&id) => StatusCode::ACCEPTED,
        Some(_) => StatusCode::NO_CONTENT,
    };
    (status, [(header::CACHE_CONTROL, "no-store")])
}

pub async fn play(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((id, seed, rules, distance)): Path<(String, i64, u8, u8)>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(worlds) = &state.worlds else { return StatusCode::NOT_FOUND.into_response() };
    if !worlds.allows(peer.ip()) || !guard::same_origin(&state.config, &headers) || !valid_id(&id) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if rules > 31 || !(2..=32).contains(&distance) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let lock_path = worlds.root.join(format!("{id}.lock"));
    let lock = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(lock_path) {
        Ok(lock) => lock,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if lock.try_lock().is_err() {
        return StatusCode::CONFLICT.into_response();
    }
    let path = worlds.root.join(&id);
    if path.symlink_metadata().is_ok_and(|meta| !meta.is_dir()) {
        return StatusCode::CONFLICT.into_response();
    }
    let client = guard::client(&state.config, ConnectInfo(peer), &headers);
    let Some(session) = worlds.reserve(id, client) else { return StatusCode::CONFLICT.into_response() };
    let (relay_read, server_write) = tokio::io::simplex(1024 * 1024);
    let (server_read, relay_write) = tokio::io::simplex(256 * 1024);
    upgrade.max_message_size(256 * 1024).on_upgrade(move |socket| async move {
        let pending = pomme_singleplayer::launch(LaunchOptions {
            save_path: path, seed, view_distance: distance, simulation_distance: distance.min(8),
            game_mode: if rules & 1 != 0 { GameType::Creative } else { GameType::Survival },
            difficulty: match (rules >> 1) & 3 {
                0 => Difficulty::Peaceful, 1 => Difficulty::Easy,
                2 => Difficulty::Normal, _ => Difficulty::Hard,
            },
            allow_commands: rules & 8 != 0,
            superflat: rules & 16 != 0,
            transport: ServerTransport { read: Box::new(server_read), write: Box::new(server_write) },
        });
        connect::bridge_stream(socket, relay_read, relay_write, &state.limits, client).await;
        // Save on a blocking thread; keep the file and in-process locks until
        // the integrated server has written players and region files.
        let _ = tokio::task::spawn_blocking(move || { drop(pending); drop(lock); drop(session); }).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_ids_are_fixed_lowercase_hex_and_sessions_are_exclusive() {
        let id = "1234567890abcdef1234567890abcdef";
        assert!(valid_id(id));
        assert!(!valid_id("../123"));
        assert!(!valid_id(&id.to_uppercase()));
        let ip: IpAddr = [127, 0, 0, 1].into();
        let worlds = Arc::new(Worlds::new(PathBuf::new(), 1, false));
        let first = worlds.reserve(id.into(), ip).unwrap();
        assert!(worlds.reserve(id.into(), ip).is_none());
        drop(first);
        assert!(worlds.reserve(id.into(), ip).is_some());
    }
}

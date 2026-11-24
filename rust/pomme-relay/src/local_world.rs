//! Explicitly local singleplayer. Each WebSocket owns one integrated
//! server and its save lock; this never exposes a public TCP game port.
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{ConnectInfo, Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use pomme_singleplayer::{Difficulty, GameType, LaunchOptions, ServerTransport};

use crate::{AppState, connect, guard};

pub struct Worlds {
    root: PathBuf,
    active: Mutex<Option<String>>,
    busy: AtomicBool,
}

impl Worlds {
    pub fn new(root: PathBuf) -> Self {
        Self { root, active: Mutex::new(None), busy: AtomicBool::new(false) }
    }

    fn reserve(self: &Arc<Self>, id: String) -> Option<Session> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return None;
        }
        *self.active.lock().unwrap() = Some(id);
        Some(Session(Arc::clone(self)))
    }
}

struct Session(Arc<Worlds>);
impl Drop for Session {
    fn drop(&mut self) {
        *self.0.active.lock().unwrap() = None;
        self.0.busy.store(false, Ordering::Release);
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
        Some(_) if !peer.ip().is_loopback() || guard::cross_site(&headers) || !valid_id(&id) => {
            StatusCode::FORBIDDEN
        }
        Some(worlds) if worlds.active.lock().unwrap().as_deref() == Some(&id) => StatusCode::ACCEPTED,
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
    if !peer.ip().is_loopback() || !guard::same_origin(&state.config, &headers) || !valid_id(&id) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if rules > 15 || !(2..=32).contains(&distance) {
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
    let Some(session) = worlds.reserve(id) else { return StatusCode::CONFLICT.into_response() };
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
            transport: ServerTransport { read: Box::new(server_read), write: Box::new(server_write) },
        });
        connect::bridge_stream(socket, relay_read, relay_write, &state.limits, peer.ip()).await;
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
        let worlds = Arc::new(Worlds::new(PathBuf::new()));
        let first = worlds.reserve(id.into()).unwrap();
        assert!(worlds.reserve(id.into()).is_none());
        drop(first);
        assert!(worlds.reserve(id.into()).is_some());
    }
}

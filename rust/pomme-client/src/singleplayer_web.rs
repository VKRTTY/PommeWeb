//! A browser world uses the same integrated server as desktop, hosted by the
//! loopback relay. The browser owns the world list; the relay owns region data.
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::world_list::{self, WorldSummary};

static AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

fn origin() -> Option<String> {
    js_sys::Reflect::get(&js_sys::global(), &"origin".into()).ok()?.as_string()
}

pub async fn detect() {
    let Some(origin) = origin() else { return };
    let url = format!("{origin}/local-world/00000000000000000000000000000000");
    let status = crate::tasks::time::timeout(Duration::from_secs(3), crate::net::http::get(&url)).await;
    AVAILABLE.store(matches!(status, Ok(Ok(response))
        if response.status() == reqwest::StatusCode::NO_CONTENT), Ordering::Relaxed);
}

pub struct World {
    dir: PathBuf,
    id: String,
    closed: Rc<Cell<bool>>,
}

impl World {
    pub fn poll(&mut self) -> Result<(), String> {
        // The game connection reports integrated-server closure.
        Ok(())
    }

    pub fn begin_close(&self) {
        let closed = Rc::clone(&self.closed);
        let path = format!("/local-world/{}", self.id);
        let _ = crate::tasks::spawn(async move {
            if let Some(origin) = origin() {
                let url = format!("{origin}{path}");
                for _ in 0..300 {
                    if !matches!(crate::net::http::get(&url).await,
                        Ok(response) if response.status() == reqwest::StatusCode::ACCEPTED)
                    {
                        break;
                    }
                    crate::tasks::time::sleep(Duration::from_millis(100)).await;
                }
            }
            closed.set(true);
        });
    }

    pub fn is_closed(&self) -> bool { self.closed.get() }

    pub fn dir(&self) -> &Path { &self.dir }
}

pub fn open(world: &WorldSummary, dir: &Path, view_distance: u8) -> Result<(World, String), String> {
    use world_list::{Difficulty, GameMode};
    if !available() { return Err(super::UNAVAILABLE_MESSAGE.to_owned()); }
    let marker = dir.join("web-world-id");
    let id = match pomme_dirs::fs::read_to_string(&marker) {
        Ok(id) => uuid::Uuid::parse_str(&id).map_err(|e| e.to_string())?.simple().to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = uuid::Uuid::new_v4().simple().to_string();
            pomme_dirs::fs::write(&marker, &id).map_err(|e| e.to_string())?;
            id
        }
        Err(e) => return Err(e.to_string()),
    };
    let seed = world_list::parse_seed(&world.seed).unwrap_or_else(|| fastrand::i64(..));
    let difficulty = match world.difficulty {
        Difficulty::Peaceful => 0,
        Difficulty::Easy => 1,
        Difficulty::Normal => 2,
        Difficulty::Hard => 3,
    };
    let rules = u8::from(matches!(world.game_mode, GameMode::Creative))
        | (difficulty << 1) | (u8::from(world.allow_commands) << 3)
        | (u8::from(world.extra.get("world_type").and_then(|v| v.as_str()) == Some("flat")) << 4);
    let distance = view_distance.clamp(2, 32);
    let endpoint = format!("/local-world/{id}/{seed}/{rules}/{distance}");
    Ok((World { dir: dir.to_owned(), id, closed: Rc::new(Cell::new(false)) }, endpoint))
}

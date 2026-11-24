//! A port of Meteor Client (github.com/MeteorDevelopment/meteor-client,
//! GPL-3.0, by MeteorDevelopment) into Pomme. Its modules, their settings
//! and defaults come from Meteor's own source (`catalog.json`, written by
//! PommeWeb's `scripts/meteor-extract.py`); the click GUI (`gui`) and HUD
//! (`hud`) follow its Meteor theme; behavior is ported module by module
//! (`ported`).
//!
//! State saves to `<game dir>/meteor-client/modules.json`, as Meteor keeps
//! its own folder in the game directory.

pub mod esp;
pub mod gui;
pub mod hud;
pub mod modules;
pub mod ported;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Value, json};
use winit::keyboard::KeyCode;

#[derive(Deserialize)]
pub struct Catalog {
    pub categories: Vec<String>,
    pub modules: Vec<ModuleInfo>,
}

#[derive(Deserialize)]
pub struct ModuleInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub groups: Vec<GroupInfo>,
}

#[derive(Deserialize)]
pub struct GroupInfo {
    pub name: String,
    pub settings: Vec<SettingInfo>,
}

#[derive(Deserialize)]
pub struct SettingInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Value,
    #[serde(default)]
    pub options: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub slider_min: Option<f64>,
    pub slider_max: Option<f64>,
}

impl SettingInfo {
    /// The slider's ends, as Meteor's number settings pick them.
    pub fn slider_range(&self) -> (f64, f64) {
        let lo = self.slider_min.or(self.min).unwrap_or(0.0);
        let hi = self.slider_max.or(self.max).unwrap_or(match self.kind.as_str() {
            "Int" | "Long" => 10.0,
            _ => 10.0,
        });
        (lo, hi.max(lo))
    }

    /// `value` held to the setting's hard limits.
    pub fn clamp(&self, value: f64) -> f64 {
        let value = self.min.map_or(value, |min| value.max(min));
        self.max.map_or(value, |max| value.min(max))
    }
}

pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("catalog.json")).expect("the Meteor catalog parses")
});

/// Modules whose behavior has been ported; the rest show and save their
/// settings like Meteor's but don't act yet.
pub const PORTED: &[&str] = &[
    "fullbright", "zoom", "sprint", "auto-walk", "auto-jump", "step", "reach", "velocity",
    "jesus", "fast-climb", "flight", "high-jump", "no-fall", "speed", "spider", "air-jump",
    "long-jump", "anti-void", "auto-clicker", "anti-afk", "auto-respawn", "notifier", "kill-aura",
    "timer", "fast-use", "break-delay", "parkour", "esp",
];

/// One module's state.
#[derive(Clone, Default)]
pub struct ModuleState {
    pub active: bool,
    pub favorite: bool,
    /// The bind, as winit names the key (`KeyR`, `ShiftRight`).
    pub bind: Option<String>,
    pub toggle_on_release: bool,
    pub chat_feedback: bool,
    /// Settings changed from their default, by group and setting name.
    pub values: HashMap<(String, String), Value>,
}

/// `Zoom`'s running state: the zoom reached and the easing time.
pub struct ZoomState {
    pub value: f64,
    pub time: f64,
}

pub struct MeteorClient {
    pub modules: Vec<ModuleState>,
    by_name: HashMap<&'static str, usize>,
    path: PathBuf,
    pub gui: gui::GuiState,
    pub hud: hud::HudState,
    pub zoom: ZoomState,
    pub memory: modules::Memory,
    /// What changed since the last save.
    dirty: bool,
    /// Toggles to announce in chat (Meteor's chat feedback).
    pub feedback: Vec<String>,
}

impl MeteorClient {
    pub fn new(game_dir: &std::path::Path) -> Self {
        let catalog = &*CATALOG;
        let by_name = catalog
            .modules
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.as_str(), i))
            .collect();
        let mut modules = vec![
            ModuleState {
                chat_feedback: true,
                ..Default::default()
            };
            catalog.modules.len()
        ];
        let path = game_dir.join("meteor-client").join("modules.json");
        let mut gui = gui::GuiState::default();
        if let Ok(text) = pomme_dirs::fs::read_to_string(&path)
            && let Ok(saved) = serde_json::from_str::<Value>(&text)
        {
            load(&saved, &by_name, &mut modules);
            gui.load(&saved["gui"]);
        }
        Self {
            modules,
            by_name,
            path,
            gui,
            hud: hud::HudState::default(),
            zoom: ZoomState { value: 1.0, time: 0.0 },
            memory: {
                let memory = modules::Memory::default();
                memory.flight_last_packet_y.set(f64::MAX);
                let mut memory = memory;
                memory.timer_override = 1.0;
                memory
            },
            dirty: false,
            feedback: Vec::new(),
        }
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.index(name).is_some_and(|i| self.modules[i].active)
    }

    pub fn toggle(&mut self, index: usize) {
        let state = &mut self.modules[index];
        state.active = !state.active;
        if state.chat_feedback {
            let info = &CATALOG.modules[index];
            self.feedback.push(format!(
                "Toggled {} {}.",
                info.title,
                if state.active { "on" } else { "off" }
            ));
        }
        self.dirty = true;
    }

    /// `Timer.getMultiplier`: an override (Speed's) wins, else the module's
    /// multiplier while it's on.
    pub fn timer_multiplier(&self) -> f64 {
        if self.memory.timer_override != 1.0 {
            self.memory.timer_override
        } else if self.is_active("timer") {
            self.get_f64("timer", "multiplier")
        } else {
            1.0
        }
    }

    /// A setting's current value: changed, or Meteor's default.
    pub fn value(&self, index: usize, group: &str, setting: &str) -> Value {
        if let Some(v) = self.modules[index]
            .values
            .get(&(group.to_owned(), setting.to_owned()))
        {
            return v.clone();
        }
        CATALOG.modules[index]
            .groups
            .iter()
            .find(|g| g.name == group)
            .and_then(|g| g.settings.iter().find(|s| s.name == setting))
            .map_or(Value::Null, |s| s.default.clone())
    }

    /// `setting` of module `name`, looked up in any group.
    pub fn get(&self, name: &str, setting: &str) -> Value {
        let Some(index) = self.index(name) else {
            return Value::Null;
        };
        CATALOG.modules[index]
            .groups
            .iter()
            .find(|g| g.settings.iter().any(|s| s.name == setting))
            .map_or(Value::Null, |g| self.value(index, &g.name, setting))
    }

    pub fn get_f64(&self, name: &str, setting: &str) -> f64 {
        self.get(name, setting).as_f64().unwrap_or(0.0)
    }

    pub fn get_bool(&self, name: &str, setting: &str) -> bool {
        self.get(name, setting).as_bool().unwrap_or(false)
    }

    pub fn set_value(&mut self, index: usize, group: &str, setting: &str, value: Value) {
        self.modules[index]
            .values
            .insert((group.to_owned(), setting.to_owned()), value);
        self.dirty = true;
    }

    pub fn reset_value(&mut self, index: usize, group: &str, setting: &str) {
        self.modules[index]
            .values
            .remove(&(group.to_owned(), setting.to_owned()));
        self.dirty = true;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Module binds pressed (`down`) or let go this frame, while playing.
    pub fn on_key(&mut self, key: KeyCode, down: bool) {
        let name = format!("{key:?}");
        for index in 0..self.modules.len() {
            let state = &self.modules[index];
            if state.bind.as_deref() != Some(&name) {
                continue;
            }
            if down || (state.toggle_on_release && state.active) {
                self.toggle(index);
            }
        }
    }

    /// Writes the state if it changed.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let modules: serde_json::Map<String, Value> = self
            .modules
            .iter()
            .zip(&CATALOG.modules)
            .map(|(state, info)| {
                let mut settings = serde_json::Map::new();
                for ((group, name), value) in &state.values {
                    settings.insert(format!("{group}/{name}"), value.clone());
                }
                (
                    info.name.clone(),
                    json!({
                        "active": state.active,
                        "favorite": state.favorite,
                        "bind": state.bind,
                        "toggle-on-release": state.toggle_on_release,
                        "chat-feedback": state.chat_feedback,
                        "settings": settings,
                    }),
                )
            })
            .collect();
        let doc = json!({ "modules": modules, "gui": self.gui.save() });
        if let Some(dir) = self.path.parent() {
            let _ = pomme_dirs::fs::create_dir_all(dir);
        }
        if let Err(e) = pomme_dirs::fs::write(&self.path, doc.to_string()) {
            tracing::warn!("Couldn't save Meteor modules: {e}");
        }
    }
}

fn load(saved: &Value, by_name: &HashMap<&'static str, usize>, modules: &mut [ModuleState]) {
    let Some(map) = saved.get("modules").and_then(Value::as_object) else {
        return;
    };
    for (name, entry) in map {
        let Some(&index) = by_name.get(name.as_str()) else {
            continue;
        };
        let state = &mut modules[index];
        state.active = entry["active"].as_bool().unwrap_or(false);
        state.favorite = entry["favorite"].as_bool().unwrap_or(false);
        state.bind = entry["bind"].as_str().map(str::to_owned);
        state.toggle_on_release = entry["toggle-on-release"].as_bool().unwrap_or(false);
        state.chat_feedback = entry["chat-feedback"].as_bool().unwrap_or(true);
        if let Some(settings) = entry["settings"].as_object() {
            for (key, value) in settings {
                if let Some((group, setting)) = key.split_once('/') {
                    state
                        .values
                        .insert((group.to_owned(), setting.to_owned()), value.clone());
                }
            }
        }
    }
}

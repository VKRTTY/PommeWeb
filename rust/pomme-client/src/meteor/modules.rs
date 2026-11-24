//! More of Meteor's modules, applied each client tick after the player's
//! movement (Meteor's `TickEvent.Post`): movement, player and combat
//! behavior over the player and entity state.

use pomme_protocol::serverbound::{ClientCommand, GamePacket};

use super::MeteorClient;
use crate::app::input::{Action, InputState};
use crate::app::phases::in_game::GameState;
use pomme_registry::builtin::EntityKind;
use crate::entity::components::LookDirection;
use crate::net::sender::PacketSender;
use crate::ui::text::TextSpan;

/// Per-module tick counters and what a module remembers between ticks.
#[derive(Default)]
pub struct Memory {
    ticks: u64,
    last_hit: u64,
    clicker_left: u32,
    clicker_right: u32,
    afk_spin: f32,
    seen: std::collections::HashSet<i32>,
    was_on_ground: bool,
    /// Actions to hold from the next frame on (AutoClicker).
    pub pending: std::collections::HashSet<Action>,
}

fn horizontal_input(input: &InputState, yaw_rad: f32) -> Option<(f64, f64)> {
    let forward = f64::from(input.action_held(Action::Forward)) - f64::from(input.action_held(Action::Backward));
    let strafe = f64::from(input.action_held(Action::StrafeLeft)) - f64::from(input.action_held(Action::StrafeRight));
    if forward == 0.0 && strafe == 0.0 {
        return None;
    }
    let (sin, cos) = (f64::from(yaw_rad).sin(), f64::from(yaw_rad).cos());
    let (x, z) = (-sin * forward + cos * strafe, cos * forward + sin * strafe);
    let len = (x * x + z * z).sqrt();
    Some((x / len, z / len))
}

pub fn tick(meteor: &mut MeteorClient, input: &InputState, game: &mut GameState, sender: &PacketSender) {
    meteor.memory.ticks += 1;
    let ticks = meteor.memory.ticks;
    let jump_pressed = input.key_just_pressed(winit::keyboard::KeyCode::Space);

    // Speed: Vanilla sets the horizontal speed in blocks per second; Strafe
    // multiplies it.
    if meteor.is_active("speed") {
        let p = &game.player;
        let ok = (!input.action_held(Action::Sneak) || meteor.get_bool("speed", "when-sneaking"))
            && (p.on_ground || !meteor.get_bool("speed", "only-on-ground"))
            && (!p.in_water || meteor.get_bool("speed", "in-liquids"))
            && !p.flying;
        if ok && let Some((dx, dz)) = horizontal_input(input, game.player.look_dir.y_rot_rad()) {
            let bps = if meteor.get("speed", "mode").as_str() == Some("Strafe") {
                meteor.get_f64("speed", "strafe-speed") * 4.3
            } else {
                meteor.get_f64("speed", "vanilla-speed")
            };
            let limit = meteor.get_bool("speed", "speed-limit");
            let per_tick = if limit { (bps / 20.0).min(0.9) } else { bps / 20.0 };
            game.player.velocity.x = dx * per_tick;
            game.player.velocity.z = dz * per_tick;
        }
    }

    // Spider: climbs walls it's pushed into.
    if meteor.is_active("spider") && game.player.horizontal_collision && !input.action_held(Action::Sneak) {
        game.player.velocity.y = meteor.get_f64("spider", "climb-speed");
    }

    // AirJump: a jump press in the air jumps again.
    if meteor.is_active("air-jump") && jump_pressed && !game.player.on_ground {
        game.player.velocity.y = 0.42;
    }

    // LongJump, Vanilla: the jump's horizontal speed boosted.
    let on_ground = game.player.on_ground;
    if meteor.is_active("long-jump")
        && meteor.get("long-jump", "mode").as_str() == Some("Vanilla")
        && meteor.memory.was_on_ground
        && !on_ground
        && game.player.velocity.y > 0.0
    {
        let boost = meteor.get_f64("long-jump", "vanilla-boost-factor");
        game.player.velocity.x *= boost;
        game.player.velocity.z *= boost;
    }
    meteor.memory.was_on_ground = on_ground;

    // AntiVoid, Jump: held up over the void.
    if meteor.is_active("anti-void") {
        let floor = game.chunk_store.min_y() as f64;
        if game.player.position.y < floor + 1.0 && game.player.velocity.y < 0.3 {
            game.player.velocity.y = 0.5;
        }
    }

    // AutoClicker: holds attack and use on Meteor's press/hold cadence.
    if meteor.is_active("auto-clicker") && game.world_has_pointer() {
        for (setting, delay, action, counter) in [
            ("mode-left", "delay-left", Action::Destroy, 0),
            ("mode-right", "delay-right", Action::Use, 1),
        ] {
            let mode = meteor.get("auto-clicker", setting);
            let wait = meteor.get_f64("auto-clicker", delay) as u32;
            let count = if counter == 0 { &mut meteor.memory.clicker_left } else { &mut meteor.memory.clicker_right };
            match mode.as_str() {
                Some("Hold") => {
                    meteor.memory.pending.insert(action);
                }
                Some("Press") => {
                    *count += 1;
                    if *count > wait {
                        *count = 0;
                        meteor.memory.pending.insert(action);
                    }
                }
                _ => {}
            }
        }
    }

    // AntiAFK: keeps the player moving.
    if meteor.is_active("anti-afk") && game.world_has_pointer() {
        if meteor.get_bool("anti-afk", "jump") && ticks % 20 == 0 && game.player.on_ground {
            game.player.velocity.y = 0.42;
        }
        if meteor.get_bool("anti-afk", "swing") && ticks % 40 == 0 {
            sender.send(GamePacket::Punch);
        }
        if meteor.get_bool("anti-afk", "spin") {
            let speed = meteor.get_f64("anti-afk", "speed") as f32;
            meteor.memory.afk_spin = (meteor.memory.afk_spin + speed) % 360.0;
            game.player.look_dir = LookDirection::new(meteor.memory.afk_spin, meteor.get_f64("anti-afk", "pitch") as f32);
        }
    }

    // FastUse and BreakDelay: their cooldowns, as Meteor sets them each tick.
    let fast_use = meteor.is_active("fast-use").then(|| meteor.get_f64("fast-use", "cooldown") as u32);
    let break_delay = meteor.is_active("break-delay").then(|| meteor.get_f64("break-delay", "cooldown") as u32);
    if fast_use.is_some() || break_delay.is_some() {
        game.interaction.cap_delays(fast_use, break_delay);
    }

    // Parkour: jumps at the edge of a block.
    if meteor.is_active("parkour")
        && game.player.on_ground
        && !input.performing_action(Action::Jump)
        && !input.action_held(Action::Sneak)
    {
        let edge = meteor.get_f64("parkour", "edge-distance");
        let reach = 0.3 - edge;
        let p = game.player.position;
        let solid_at = |x: f64, z: f64| {
            let state = game.chunk_store.get_block_state(x.floor() as i32, (p.y - 0.5).floor() as i32, z.floor() as i32);
            let id = game.block_registry.block_state_data(state).id();
            !id.ends_with("air")
        };
        let supported = [(-reach, -reach), (reach, -reach), (-reach, reach), (reach, reach)]
            .into_iter()
            .any(|(dx, dz)| solid_at(p.x + dx, p.z + dz));
        if !supported {
            game.player.velocity.y = 0.42;
        }
    }

    // AutoRespawn.
    if meteor.is_active("auto-respawn") && game.dead && game.death_screen_open {
        sender.send(GamePacket::ClientCommand(ClientCommand::PerformRespawn));
    }

    // Notifier, visual range: who comes into range.
    if meteor.is_active("notifier") && meteor.get_bool("notifier", "visual-range") {
        let current: std::collections::HashSet<i32> = game
            .entity_store
            .living
            .iter()
            .filter(|(_, e)| e.entity_type == EntityKind::Player)
            .map(|(id, _)| *id)
            .collect();
        let event = meteor.get("notifier", "event").as_str().unwrap_or("Both").to_owned();
        let mut messages = Vec::new();
        for id in current.difference(&meteor.memory.seen) {
            if event != "Despawn"
                && let Some(e) = game.entity_store.living.get(id)
            {
                messages.push(format!("{} has entered your visual range!", e.profile_name.clone().unwrap_or_default()));
            }
        }
        if event != "Spawn" {
            for _ in meteor.memory.seen.difference(&current) {
                messages.push("A player has left your visual range.".to_owned());
            }
        }
        meteor.memory.seen = current;
        for message in messages {
            game.chat.push_message(vec![
                TextSpan::new("[Meteor] ".into(), [145.0 / 255.0, 61.0 / 255.0, 226.0 / 255.0, 1.0]),
                TextSpan::new(message, [1.0, 1.0, 1.0, 1.0]),
            ]);
        }
    }

    // KillAura: hits the nearest targets in range on its delay.
    if meteor.is_active("kill-aura") && game.world_has_pointer() && !game.dead {
        let range = meteor.get_f64("kill-aura", "range");
        let delay = if meteor.get_bool("kill-aura", "custom-delay") {
            meteor.get_f64("kill-aura", "hit-delay") as u64
        } else {
            13
        };
        let eye = game.player.eye_pos();
        let own = game.player.entity_id;
        let ignore_passive = meteor.get_bool("kill-aura", "ignore-passive");
        let mut targets: Vec<(f64, i32, glam::DVec3)> = game
            .entity_store
            .living
            .iter()
            .filter(|(id, e)| **id != own && (e.entity_type == EntityKind::Player || !ignore_passive))
            .map(|(id, e)| {
                let center = glam::DVec3::new(e.position.x, e.position.y + 0.9, e.position.z);
                (center.distance(glam::DVec3::new(eye.x, eye.y, eye.z)), *id, center)
            })
            .filter(|(d, _, _)| *d <= range)
            .collect();
        targets.sort_by(|a, b| a.0.total_cmp(&b.0));
        let max = meteor.get_f64("kill-aura", "max-targets").max(1.0) as usize;
        if let Some(&(_, _, at)) = targets.first()
            && meteor.get("kill-aura", "rotate").as_str() != Some("None")
        {
            let (dx, dy, dz) = (at.x - eye.x, at.y - eye.y, at.z - eye.z);
            let yaw = (-dx).atan2(dz).to_degrees() as f32;
            let pitch = (-dy).atan2((dx * dx + dz * dz).sqrt()).to_degrees() as f32;
            game.player.look_dir = LookDirection::new(yaw, pitch);
        }
        if ticks.saturating_sub(meteor.memory.last_hit) >= delay {
            for &(_, id, _) in targets.iter().take(max) {
                sender.send_raw(pomme_protocol::wire::encode_attack(id));
                sender.send(GamePacket::Punch);
            }
            if !targets.is_empty() {
                meteor.memory.last_hit = ticks;
            }
        }
    }
}

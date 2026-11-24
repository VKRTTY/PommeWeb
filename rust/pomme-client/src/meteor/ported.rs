//! The modules whose behavior is ported, applied each frame: what they hold
//! down for the player, and their toggles announced in chat.

use super::MeteorClient;
use crate::attribute::AttributeKind;
use crate::app::input::{Action, InputState};
use crate::app::phases::in_game::GameState;
use crate::ui::text::TextSpan;

/// Meteor's chat prefix color.
const PREFIX: [f32; 4] = [145.0 / 255.0, 61.0 / 255.0, 226.0 / 255.0, 1.0];
const GRAY: [f32; 4] = [170.0 / 255.0, 170.0 / 255.0, 170.0 / 255.0, 1.0];

pub fn apply(meteor: &mut MeteorClient, input: &mut InputState, game: &mut GameState) {
    for message in meteor.feedback.drain(..) {
        game.chat.push_message(vec![
            TextSpan::new("[Meteor] ".into(), PREFIX),
            TextSpan::new(message, GRAY),
        ]);
    }

    input.forced_actions.clear();
    input.forced_actions.extend(meteor.memory.pending.drain());
    if !game.world_has_pointer() {
        return;
    }
    let player = &game.player;
    let moving = input.action_held(Action::Forward)
        || input.action_held(Action::Backward)
        || input.action_held(Action::StrafeLeft)
        || input.action_held(Action::StrafeRight);

    // AutoWalk: Simple walks the chosen way; Smart, which paths with
    // Baritone in Meteor, walks forward until Pomme has pathing.
    if meteor.is_active("auto-walk") {
        let direction = match meteor.get("auto-walk", "mode").as_str() {
            Some("Simple") => meteor.get("auto-walk", "simple-direction").as_str().unwrap_or("Forwards").to_owned(),
            _ => "Forwards".to_owned(),
        };
        let action = match direction.as_str() {
            "Backwards" => Action::Backward,
            "Left" => Action::StrafeLeft,
            "Right" => Action::StrafeRight,
            _ => Action::Forward,
        };
        input.forced_actions.insert(action);
    }

    // Sprint, Strict: sprint whenever vanilla allows it while moving forward.
    if meteor.is_active("sprint") {
        let in_water = player.eyes_in_water;
        let unsprint_in_water = meteor.get_bool("sprint", "unsprint-in-water");
        let stationary = meteor.get_bool("sprint", "sprint-while-stationary");
        if (moving || stationary || meteor.is_active("auto-walk")) && !(in_water && unsprint_in_water) {
            input.forced_actions.insert(Action::Sprint);
        }
    }

    // AutoJump: jumps on the ground when its condition holds.
    if meteor.is_active("auto-jump") && player.on_ground && !input.action_held(Action::Sneak) {
        let walking = moving || meteor.is_active("auto-walk");
        let jump = match meteor.get("auto-jump", "jump-if").as_str() {
            Some("Sprinting") => walking && input.performing_action(Action::Sprint),
            Some("Walking") => walking,
            _ => true,
        };
        if jump {
            input.forced_actions.insert(Action::Jump);
        }
    }
}

/// Per-tick module behavior, run after each client tick (Meteor's
/// `TickEvent.Post`): velocity, abilities and attribute changes.
pub fn tick(meteor: &mut MeteorClient, input: &InputState, game: &mut GameState) {
    let player = &mut game.player;
    let jump = input.performing_action(Action::Jump);
    let sneak = input.action_held(Action::Sneak);

    // Step: raises the step height to the setting, while its condition holds.
    let step = if meteor.is_active("step") {
        let when = meteor.get("step", "active-when");
        let work = match when.as_str() {
            Some("Sneaking") => sneak,
            Some("NotSneaking") => !sneak,
            _ => true,
        };
        let height = meteor.get_f64("step", "height");
        let base = player.attributes.instance(AttributeKind::StepHeight).map_or(0.6, |i| i.base_value());
        (work && height > base).then(|| height / base - 1.0)
    } else {
        None
    };
    player.attributes.set_modifier(AttributeKind::StepHeight, "meteor:step", step);

    // Reach: the interaction ranges, set to the settings.
    for (attribute, id, setting, base) in [
        (AttributeKind::BlockInteractionRange, "meteor:reach-blocks", "block-reach", 4.5),
        (AttributeKind::EntityInteractionRange, "meteor:reach-entities", "entity-reach", 3.0),
    ] {
        let amount = meteor
            .is_active("reach")
            .then(|| meteor.get_f64("reach", setting) / base - 1.0)
            .filter(|a| a.abs() > 1e-9);
        player.attributes.set_modifier(attribute, id, amount);
    }

    // Velocity, sinking: no sinking in fluid unless a vertical key is held.
    if meteor.is_active("velocity")
        && meteor.get_bool("velocity", "sinking")
        && !jump
        && !sneak
        && player.in_water
        && player.velocity.y < 0.0
    {
        player.velocity.y = 0.0;
    }

    // Jesus: rising in fluid, as `Jesus.onTick`, until it holds the player
    // at the surface.
    if meteor.is_active("jesus") && !player.flying && player.in_water {
        player.velocity.y = 0.11;
    }

    // FastClimb.
    if meteor.is_active("fast-climb")
        && !meteor.get_bool("fast-climb", "timer-mode")
        && (player.horizontal_collision || jump)
        && crate::physics::movement::on_climbable(player, &game.chunk_store, game.block_registry.block_state_table(), game.block_registry.tags())
    {
        player.velocity.y = meteor.get_f64("fast-climb", "speed");
    }

    // Flight, as `Flight.java`: the yaw nudge while hanging still, the anti-kick
    // counters, then the mode's movement.
    {
        let on = meteor.is_active("flight");
        let abilities = meteor.get("flight", "mode").as_str() == Some("Abilities");
        let speed = meteor.get_f64("flight", "speed");
        let creative = player.game_mode == 1;
        if on && !meteor.memory.flight_was_on {
            meteor.memory.flight_last_packet_y.set(f64::MAX);
            meteor.memory.flight_delay_left = meteor.get_f64("flight", "delay") as i32;
            meteor.memory.flight_off_left = meteor.get_f64("flight", "off-time") as i32;
        }
        if !on && meteor.memory.flight_was_on {
            // `onDeactivate`: Abilities mode gives the abilities back.
            if meteor.get("flight", "mode").as_str() == Some("Abilities") && player.flying {
                player.flying = false;
                player.abilities_dirty = true;
                player.fly_speed = 0.05;
                if !creative {
                    player.may_fly = false;
                }
            }
        }
        meteor.memory.flight_was_on = on;
        player.meteor_air_speed = None;
        if on {
            // onPreTick: a yaw flip every tick the player hangs motionless
            // after a long fall.
            let yaw = player.look_dir.y_rot_deg();
            let still = (player.velocity.x.powi(2) + player.velocity.y.powi(2) + player.velocity.z.powi(2)).sqrt() < 0.003;
            if player.fall_distance >= 3.0 && yaw == meteor.memory.flight_last_yaw && still {
                let flip = if meteor.memory.flight_flip { 1.0 } else { -1.0 };
                player.look_dir = crate::entity::components::LookDirection::new(yaw + flip, player.look_dir.x_rot_deg());
                meteor.memory.flight_flip = !meteor.memory.flight_flip;
            }
            meteor.memory.flight_last_yaw = yaw;

            // onPostTick: the delay and off-time of the anti-kick.
            let anti_kick = meteor.index("flight").map(|i| meteor.value(i, "Anti Kick", "mode"));
            let anti_kick = anti_kick.as_ref().and_then(|v| v.as_str()).unwrap_or("None").to_owned();
            let (delay, off_time) = (meteor.get_f64("flight", "delay") as i32, meteor.get_f64("flight", "off-time") as i32);
            if meteor.memory.flight_delay_left > 0 {
                meteor.memory.flight_delay_left -= 1;
            }
            let mut skip = false;
            if meteor.memory.flight_off_left <= 0 && meteor.memory.flight_delay_left <= 0 {
                meteor.memory.flight_delay_left = delay;
                meteor.memory.flight_off_left = off_time;
                if anti_kick == "Packet" {
                    // `setPositionReminder(20)`: a position packet next tick.
                    game.position_send_counter = 20;
                }
            } else if meteor.memory.flight_delay_left <= 0 {
                if anti_kick == "Normal" {
                    if abilities {
                        // abilitiesOff: flying off for the off-time.
                        player.flying = false;
                        player.abilities_dirty = true;
                        player.fly_speed = 0.05;
                        if !creative {
                            player.may_fly = false;
                        }
                        skip = true;
                    }
                } else if anti_kick == "Packet" && meteor.memory.flight_off_left == off_time {
                    game.position_send_counter = 20;
                }
                meteor.memory.flight_off_left -= 1;
            }
            if !skip {
                if abilities {
                    if player.game_mode != 3 {
                        player.fly_speed = speed as f32;
                        if !player.flying {
                            player.flying = true;
                            player.abilities_dirty = true;
                        }
                        if !creative {
                            player.may_fly = true;
                        }
                    }
                } else {
                    // Velocity: no flying, all motion cleared, the vertical keys
                    // add; the horizontal push is the air acceleration
                    // `getFlyingSpeed` returns, `speed * 10` (15 sprinting).
                    player.flying = false;
                    player.velocity.x = 0.0;
                    player.velocity.y = 0.0;
                    player.velocity.z = 0.0;
                    let step = speed * if meteor.get_bool("flight", "vertical-speed-match") { 10.0 } else { 5.0 };
                    if jump {
                        player.velocity.y += step;
                    }
                    if sneak {
                        player.velocity.y -= step;
                    }
                    if meteor.get_bool("flight", "no-sneak") {
                        player.on_ground = false;
                    }
                    player.meteor_air_speed = Some(speed as f32 * if player.sprinting { 15.0 } else { 10.0 });
                }
            }
        }
    }

    // HighJump: extra upward velocity on a jump from the ground.
    if meteor.is_active("high-jump") && jump && player.on_ground {
        player.velocity.y += meteor.get_f64("high-jump", "multiplier") * 0.1;
    }
}

/// `Zoom`: the scroll wheel changes the zoom, and the view eases in and out
/// over a fifth of a second; returns the divisor to apply to the FOV.
pub fn zoom_scaling(meteor: &mut MeteorClient, input: &mut InputState, dt: f32) -> f64 {
    let active = meteor.is_active("zoom");
    let z = &mut meteor.zoom;
    let target = meteor.get_f64("zoom", "zoom").max(1.0);
    input.scroll_taken = active && meteor.get_f64("zoom", "scroll-sensitivity") > 0.0;
    let wheel = std::mem::take(&mut input.zoom_scroll) as f64;
    let z = &mut meteor.zoom;
    if active && z.time == 0.0 {
        z.value = target;
        z.time = 0.001;
    }
    let sensitivity = meteor.get_f64("zoom", "scroll-sensitivity");
    let smooth = meteor.get_bool("zoom", "smooth");
    let z = &mut meteor.zoom;
    if active && sensitivity > 0.0 && wheel != 0.0 {
        z.value = (z.value + wheel * 0.25 * (sensitivity * z.value)).max(1.0);
    }
    if smooth {
        z.time += f64::from(dt) * 5.0 * if active { 1.0 } else { -1.0 };
        z.time = z.time.clamp(0.0, 1.0);
    } else {
        z.time = if active { 1.0 } else { 0.0 };
    }
    let t = z.time;
    // Ease in out cubic.
    let delta = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
    1.0 + (z.value - 1.0) * delta
}

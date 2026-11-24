//! The modules whose behavior is ported, applied each frame: what they hold
//! down for the player, and their toggles announced in chat.

use super::MeteorClient;
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

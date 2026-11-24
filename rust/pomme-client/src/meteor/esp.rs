//! Meteor's ESP: a box around each entity. Meteor's 2D mode draws a screen
//! rectangle from the entity's projected hitbox corners, which every mode
//! does here until the world pass can draw 3D boxes.

use glam::DVec3;
use pomme_registry::builtin::EntityKind;

use super::MeteorClient;
use crate::app::phases::in_game::GameState;
use crate::entity::dimensions::EntityDimensions;
use crate::renderer::Renderer;
use crate::renderer::pipelines::menu_overlay::MenuElement;

fn color(meteor: &MeteorClient, setting: &str, fade: f32) -> [f32; 4] {
    let c = meteor.get("esp", setting);
    let c = c.as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(255.0) as f32 / 255.0).collect::<Vec<_>>()).unwrap_or_default();
    if c.len() >= 4 { [c[0], c[1], c[2], c[3] * fade] } else { [1.0, 1.0, 1.0, fade] }
}

pub fn build(meteor: &MeteorClient, game: &GameState, renderer: &Renderer, elements: &mut Vec<MenuElement>, sw: f32, sh: f32) {
    if !meteor.is_active("esp") {
        return;
    }
    let width = (meteor.get_f64("esp", "outline-width") as f32).max(1.0) * (sh / 1080.0).max(1.0);
    let fill = meteor.get_f64("esp", "fill-opacity") as f32;
    let shape = meteor.get("esp", "shape-mode");
    let (lines, sides) = match shape.as_str() {
        Some("Lines") => (true, false),
        Some("Sides") => (false, true),
        _ => (true, true),
    };
    let eye = game.player.eye_pos();
    let eye = DVec3::new(eye.x, eye.y, eye.z);
    let fade_distance = meteor.get_f64("esp", "fade-distance");
    for (id, entity) in &game.entity_store.living {
        if *id == game.player.entity_id && meteor.get_bool("esp", "ignore-self") {
            continue;
        }
        let dims = EntityDimensions::of(entity.entity_type);
        let (half, height) = (f64::from(dims.width) / 2.0, f64::from(dims.height));
        let base = DVec3::new(entity.position.x, entity.position.y, entity.position.z);
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut visible = true;
        for dx in [-half, half] {
            for dy in [0.0, height] {
                for dz in [-half, half] {
                    match renderer.world_to_screen(base + DVec3::new(dx, dy, dz), sw, sh) {
                        Some((x, y)) => {
                            min_x = min_x.min(x);
                            max_x = max_x.max(x);
                            min_y = min_y.min(y);
                            max_y = max_y.max(y);
                        }
                        None => visible = false,
                    }
                }
            }
        }
        if !visible || max_x - min_x > sw * 4.0 {
            continue;
        }
        // Meteor fades a box in over `fade-distance` blocks.
        let distance = base.distance(eye);
        let fade = if fade_distance > 0.0 { (distance / fade_distance).min(1.0) as f32 } else { 1.0 };
        let setting = if entity.entity_type == EntityKind::Player {
            "players-color"
        } else {
            "misc-color"
        };
        let c = color(meteor, setting, fade);
        let (w, h) = (max_x - min_x, max_y - min_y);
        let rect = |x: f32, y: f32, w: f32, h: f32, color: [f32; 4]| MenuElement::Rect {
            x,
            y,
            w,
            h,
            corner_radius: 0.0,
            color,
        };
        if sides {
            elements.push(rect(min_x, min_y, w, h, [c[0], c[1], c[2], fill * fade]));
        }
        if lines {
            elements.push(rect(min_x, min_y, w, width, c));
            elements.push(rect(min_x, max_y - width, w, width, c));
            elements.push(rect(min_x, min_y, width, h, c));
            elements.push(rect(max_x - width, min_y, width, h, c));
        }
    }
}

//! Meteor's default HUD (`Hud.resetToDefaultElements`): the watermark, FPS,
//! ping and speed at the top left, the active modules at the top right in
//! rainbow colors, and the position, opposite-dimension position and
//! rotation at the bottom right.

use super::{CATALOG, MeteorClient};
use crate::renderer::pipelines::menu_overlay::MenuElement;

/// Meteor's HUD text colors: plain, then `#1`.
const PRIMARY: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SECONDARY: [f32; 4] = [175.0 / 255.0, 175.0 / 255.0, 175.0 / 255.0, 1.0];

/// What the HUD shows of the game.
pub struct HudInfo<'a> {
    pub fps: u32,
    pub ping: Option<i32>,
    /// Blocks per second, horizontally.
    pub speed: f64,
    pub pos: glam::DVec3,
    /// `minecraft:overworld`, `minecraft:the_nether`, ...
    pub dimension: &'a str,
    pub yaw: f32,
    pub pitch: f32,
}

pub struct HudState {
    /// `ActiveModulesHud.rainbowHue1`.
    hue: f64,
}

impl Default for HudState {
    fn default() -> Self {
        Self { hue: 0.0 }
    }
}

/// Text in parts, each `(text, color)`, drawn left to right.
fn line(elements: &mut Vec<MenuElement>, parts: &[(&str, [f32; 4])], x: f32, y: f32, scale: f32, width: &dyn Fn(&str, f32) -> f32) -> f32 {
    let mut cx = x;
    for (text, color) in parts {
        elements.push(MenuElement::Text {
            x: cx,
            y,
            text: (*text).to_owned(),
            scale,
            color: *color,
            centered: false,
        });
        cx += width(text, scale);
    }
    cx - x
}

fn line_width(parts: &[(&str, [f32; 4])], scale: f32, width: &dyn Fn(&str, f32) -> f32) -> f32 {
    parts.iter().map(|(t, _)| width(t, scale)).sum()
}

/// `java.awt.Color.HSBtoRGB` at full saturation and brightness.
fn hue_rgb(hue: f64) -> [f32; 4] {
    let h = (hue.rem_euclid(1.0) * 6.0) as f32;
    let f = h.fract();
    let (r, g, b) = match h as u32 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    [r, g, b, 1.0]
}

fn direction(yaw: f32) -> &'static str {
    match (((yaw.rem_euclid(360.0)) + 45.0) / 90.0) as u32 % 4 {
        0 => "South",
        1 => "West",
        2 => "North",
        _ => "East",
    }
}

pub fn build(
    meteor: &mut MeteorClient,
    elements: &mut Vec<MenuElement>,
    sw: f32,
    sh: f32,
    info: &HudInfo,
    width: &dyn Fn(&str, f32) -> f32,
) {
    let u = (sh / 1080.0).max(1.0);
    let scale = 12.0 * u;
    let h = (scale + 2.0 * u).ceil();
    let pad = 4.0 * u;

    // Top left.
    let fps = info.fps.to_string();
    let ping = info.ping.map_or_else(|| "-".to_owned(), |p| p.to_string());
    let speed = format!("{:.1}", info.speed);
    let rows: [&[(&str, [f32; 4])]; 4] = [
        &[("Meteor Client ", PRIMARY), ("1.21-pomme", SECONDARY)],
        &[("FPS: ", PRIMARY), (&fps, SECONDARY)],
        &[("Ping: ", PRIMARY), (&ping, SECONDARY)],
        &[("Speed: ", PRIMARY), (&speed, SECONDARY)],
    ];
    for (i, row) in rows.iter().enumerate() {
        line(elements, row, pad, pad + h * i as f32, scale, width);
    }

    // Top right: `ActiveModulesHud`, biggest first, rainbow.
    let mut active: Vec<&str> = (0..meteor.modules.len())
        .filter(|&i| meteor.modules[i].active)
        .map(|i| CATALOG.modules[i].title.as_str())
        .collect();
    active.sort_by(|a, b| width(b, scale).total_cmp(&width(a, scale)));
    let speed = 0.05;
    let spread = 0.01;
    meteor.hud.hue = (meteor.hud.hue + speed * 0.1).rem_euclid(1.0);
    let mut hue = meteor.hud.hue;
    for (i, title) in active.iter().enumerate() {
        hue += spread;
        let w = width(title, scale);
        line(elements, &[(title, hue_rgb(hue))], sw - pad - w, pad + h * i as f32, scale, width);
    }

    // Bottom right.
    let p = info.pos;
    let pos = format!("{}, {}, {}", p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64);
    let (other, factor) = match info.dimension {
        "minecraft:the_nether" => (Some("Overworld:"), 8.0),
        "minecraft:the_end" => (None, 1.0),
        _ => (Some("Nether:"), 0.125),
    };
    let opposite = format!(
        "{}, {}, {}",
        (p.x * factor).floor() as i64,
        p.y.floor() as i64,
        (p.z * factor).floor() as i64
    );
    let rotation = format!("({:.1}, {:.1})", info.yaw, info.pitch);
    let mut bottom: Vec<Vec<(&str, [f32; 4])>> = vec![
        vec![("Pos: ", PRIMARY), (&pos, SECONDARY)],
    ];
    if let Some(other) = other {
        bottom.push(vec![(other, PRIMARY), (" ", PRIMARY), (&opposite, SECONDARY)]);
    }
    bottom.push(vec![(direction(info.yaw), PRIMARY), (" ", PRIMARY), (&rotation, SECONDARY)]);
    for (i, row) in bottom.iter().enumerate() {
        let w = line_width(row, scale, width);
        line(elements, row, sw - pad - w, sh - pad - h * (i as f32 + 1.0), scale, width);
    }
}

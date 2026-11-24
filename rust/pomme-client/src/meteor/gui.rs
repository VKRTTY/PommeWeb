//! Meteor's click GUI, after its Meteor theme (`MeteorGuiTheme` and its
//! widgets): the tab bar, a window per category with its modules, and a
//! module's settings window. Drawn in screen pixels, as Meteor's GUI is, at
//! its default scale.

use std::collections::HashMap;

use serde_json::{Value, json};
use winit::keyboard::KeyCode;

use super::{CATALOG, MeteorClient, SettingInfo};
use crate::renderer::pipelines::menu_overlay::MenuElement;

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

// `MeteorGuiTheme`'s default colors.
const ACCENT: [f32; 4] = rgba(145, 61, 226, 255);
const MINUS: [f32; 4] = rgba(255, 50, 50, 255);
const FAVORITE: [f32; 4] = rgba(250, 215, 0, 255);
const TEXT: [f32; 4] = rgba(255, 255, 255, 255);
const TEXT_SECONDARY: [f32; 4] = rgba(150, 150, 150, 255);
const BACKGROUND: [[f32; 4]; 3] = [rgba(20, 20, 20, 200), rgba(30, 30, 30, 200), rgba(40, 40, 40, 200)];
const MODULE_BACKGROUND: [f32; 4] = rgba(50, 50, 50, 255);
const OUTLINE: [[f32; 4]; 3] = [rgba(0, 0, 0, 255), rgba(10, 10, 10, 255), rgba(20, 20, 20, 255)];
const SEPARATOR_EDGES: [f32; 4] = rgba(225, 225, 225, 150);
const SLIDER_HANDLE: [[f32; 4]; 3] = [rgba(130, 0, 255, 255), rgba(140, 30, 255, 255), rgba(150, 60, 255, 255)];
const SLIDER_LEFT: [f32; 4] = rgba(100, 35, 170, 255);
const SLIDER_RIGHT: [f32; 4] = rgba(50, 50, 50, 255);

/// Meteor's tabs, in its order.
const TABS: [&str; 7] = ["Modules", "Config", "GUI", "HUD", "Friends", "Macros", "Profiles"];

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Modules,
    Module(usize),
    /// Another tab, not yet ported.
    Tab(usize),
}

#[derive(Clone, Copy)]
struct WindowPos {
    x: f32,
    y: f32,
    expanded: bool,
    scroll: f32,
}

#[derive(Default)]
pub struct GuiState {
    pub screen: Screen,
    windows: HashMap<String, WindowPos>,
    /// The window being dragged, and the cursor's offset into it.
    drag: Option<(String, f32, f32)>,
    /// The slider being dragged: module, group, setting.
    slider: Option<(usize, String, String)>,
    /// Listening for a module's bind.
    binding: Option<usize>,
    settings_scroll: f32,
}

impl GuiState {
    pub fn save(&self) -> Value {
        let windows: serde_json::Map<String, Value> = self
            .windows
            .iter()
            .map(|(id, w)| (id.clone(), json!({"x": w.x, "y": w.y, "expanded": w.expanded})))
            .collect();
        json!({ "windows": windows })
    }

    pub fn load(&mut self, saved: &Value) {
        let Some(windows) = saved["windows"].as_object() else {
            return;
        };
        for (id, w) in windows {
            self.windows.insert(
                id.clone(),
                WindowPos {
                    x: w["x"].as_f64().unwrap_or(0.0) as f32,
                    y: w["y"].as_f64().unwrap_or(0.0) as f32,
                    expanded: w["expanded"].as_bool().unwrap_or(true),
                    scroll: 0.0,
                },
            );
        }
    }

}

/// One frame's input for the GUI.
pub struct Frame<'a> {
    pub sw: f32,
    pub sh: f32,
    pub cursor: (f32, f32),
    pub left_pressed: bool,
    pub left_down: bool,
    pub right_pressed: bool,
    pub scroll: f32,
    pub keys_pressed: &'a [KeyCode],
    pub text_width: &'a dyn Fn(&str, f32) -> f32,
}

pub enum GuiAction {
    None,
    Close,
}

/// Meteor's sizes in screen pixels: `theme.scale(1)` at its default scale,
/// grown on large screens as a 1080p screen's are.
struct Theme {
    u: f32,
    text_h: f32,
}

impl Theme {
    fn new(sh: f32) -> Self {
        let u = (sh / 1080.0).max(1.0);
        Self { u, text_h: 12.0 * u }
    }

    fn s(&self, v: f32) -> f32 {
        v * self.u
    }
}

struct Ui<'a, 'b> {
    elements: &'a mut Vec<MenuElement>,
    frame: &'a Frame<'b>,
    theme: Theme,
}

impl Ui<'_, '_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.elements.push(MenuElement::Rect {
            x,
            y,
            w,
            h,
            corner_radius: 0.0,
            color,
        });
    }

    fn text(&mut self, text: &str, x: f32, y: f32, color: [f32; 4]) {
        self.elements.push(MenuElement::TextFlat {
            x,
            y,
            text: text.to_owned(),
            scale: self.theme.text_h,
            color,
        });
    }

    fn text_w(&self, text: &str) -> f32 {
        (self.frame.text_width)(text, self.theme.text_h)
    }

    fn hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (cx, cy) = self.frame.cursor;
        cx >= x && cx < x + w && cy >= y && cy < y + h
    }

    fn state(&self, x: f32, y: f32, w: f32, h: f32) -> usize {
        match (self.hovered(x, y, w, h), self.frame.left_down) {
            (true, true) => 2,
            (true, false) => 1,
            _ => 0,
        }
    }

    /// `WTriangle`: pointing down when expanded, right when not.
    fn triangle(&mut self, x: f32, y: f32, size: f32, expanded: bool, color: [f32; 4]) {
        let rows = (size / self.theme.u).round().max(2.0) as usize / 2;
        let step = size / (rows * 2) as f32;
        for i in 0..rows {
            let inset = i as f32 * step;
            if expanded {
                self.rect(x + inset, y + step * (i as f32 + 0.5), size - inset * 2.0, step, color);
            } else {
                self.rect(x + step * (i as f32 + 0.5), y + inset, step, size - inset * 2.0, color);
            }
        }
    }

    fn checkbox(&mut self, x: f32, y: f32, checked: bool) -> bool {
        let size = self.theme.text_h + self.theme.s(4.0);
        let state = self.state(x, y, size, size);
        let border = self.theme.s(1.0);
        self.rect(x, y, size, size, OUTLINE[state]);
        self.rect(x + border, y + border, size - border * 2.0, size - border * 2.0, BACKGROUND[state]);
        if checked {
            let inset = self.theme.s(3.0);
            self.rect(x + inset, y + inset, size - inset * 2.0, size - inset * 2.0, ACCENT);
        }
        state > 0 && self.frame.left_pressed
    }

    fn button(&mut self, label: &str, x: f32, y: f32, w: f32) -> bool {
        let pad = self.theme.s(4.0);
        let h = self.theme.text_h + pad * 2.0;
        let state = self.state(x, y, w, h);
        self.rect(x, y, w, h, OUTLINE[state]);
        let b = self.theme.s(1.0);
        self.rect(x + b, y + b, w - b * 2.0, h - b * 2.0, BACKGROUND[state]);
        let tw = self.text_w(label);
        self.text(label, x + (w - tw) / 2.0, y + pad, TEXT);
        state > 0 && self.frame.left_pressed
    }
}

fn row_h(theme: &Theme) -> f32 {
    theme.text_h + theme.s(8.0)
}

pub fn build(meteor: &mut MeteorClient, elements: &mut Vec<MenuElement>, frame: &Frame) -> GuiAction {
    let mut gui = std::mem::take(&mut meteor.gui);
    let mut ui = Ui {
        elements,
        frame,
        theme: Theme::new(frame.sh),
    };
    let mut action = GuiAction::None;

    // A bind being captured takes the next key; Escape or Backspace clear.
    if let Some(index) = gui.binding
        && let Some(&key) = frame.keys_pressed.first()
    {
        meteor.modules[index].bind = match key {
            KeyCode::Escape | KeyCode::Backspace | KeyCode::Delete => None,
            key => Some(format!("{key:?}")),
        };
        meteor.mark_dirty();
        gui.binding = None;
    } else if frame.keys_pressed.contains(&KeyCode::Escape) {
        action = match gui.screen {
            Screen::Module(_) => {
                gui.screen = Screen::Modules;
                GuiAction::None
            }
            _ => GuiAction::Close,
        };
    }

    let top = top_bar(&mut ui, &mut gui);
    match gui.screen {
        Screen::Modules => modules_screen(&mut ui, &mut gui, meteor, top),
        Screen::Module(index) => module_screen(&mut ui, &mut gui, meteor, index),
        Screen::Tab(tab) => {
            let message = format!("{} isn't ported yet.", TABS[tab]);
            let w = ui.text_w(&message);
            ui.text(&message, (frame.sw - w) / 2.0, frame.sh / 2.0, TEXT_SECONDARY);
        }
    }
    if !frame.left_down {
        gui.drag = None;
        gui.slider = None;
    }
    meteor.gui = gui;
    meteor.save();
    action
}

/// `WTopBar`, centered at the top.
fn top_bar(ui: &mut Ui, gui: &mut GuiState) -> f32 {
    let pad = ui.theme.s(6.0);
    let h = ui.theme.text_h + pad * 2.0;
    let widths: Vec<f32> = TABS.iter().map(|t| ui.text_w(t) + pad * 2.0).collect();
    let mut x = (ui.frame.sw - widths.iter().sum::<f32>()) / 2.0;
    for (i, (name, w)) in TABS.iter().zip(widths).enumerate() {
        let selected = match gui.screen {
            Screen::Modules | Screen::Module(_) => i == 0,
            Screen::Tab(t) => t == i,
        };
        let state = ui.state(x, 0.0, w, h);
        ui.rect(x, 0.0, w, h, BACKGROUND[if selected { 2 } else { state }]);
        ui.text(name, x + pad, pad, TEXT);
        if state > 0 && ui.frame.left_pressed {
            gui.screen = if i == 0 { Screen::Modules } else { Screen::Tab(i) };
        }
        x += w;
    }
    h
}

/// `ModulesScreen`: a window per category, laid out in a row as
/// `WCategoryController` does, and the help text.
fn modules_screen(ui: &mut Ui, gui: &mut GuiState, meteor: &mut MeteorClient, top: f32) {
    let theme_pad = ui.theme.s(4.0);
    let row = row_h(&ui.theme);
    let header = ui.theme.text_h + ui.theme.s(8.0);

    let mut windows: Vec<(String, Vec<usize>)> = CATALOG
        .categories
        .iter()
        .map(|c| {
            let modules = (0..CATALOG.modules.len())
                .filter(|&i| &CATALOG.modules[i].category == c)
                .collect();
            (c.clone(), modules)
        })
        .collect();
    let favorites: Vec<usize> = (0..meteor.modules.len()).filter(|&i| meteor.modules[i].favorite).collect();
    if !favorites.is_empty() {
        windows.push(("Favorites".into(), favorites));
    }

    let widths: Vec<f32> = windows
        .iter()
        .map(|(name, modules)| {
            let titles = modules
                .iter()
                .map(|&i| ui.text_w(&CATALOG.modules[i].title))
                .fold(0.0, f32::max);
            (titles + theme_pad * 2.0).max(ui.text_w(name) + theme_pad * 4.0 + ui.theme.text_h)
        })
        .collect();

    // Windows not placed yet go where `WCategoryController` puts them.
    {
        let mut x = theme_pad;
        let mut y = top + theme_pad;
        for ((name, _), w) in windows.iter().zip(&widths) {
            if x + w > ui.frame.sw {
                x = theme_pad;
                y += ui.theme.s(40.0);
            }
            gui.windows.entry(name.clone()).or_insert(WindowPos {
                x,
                y,
                expanded: true,
                scroll: 0.0,
            });
            x += w + theme_pad;
        }
    }

    // Dragging moves the window held.
    if let Some((id, dx, dy)) = gui.drag.clone()
        && let Some(pos) = gui.windows.get_mut(&id)
    {
        pos.x = ui.frame.cursor.0 - dx;
        pos.y = ui.frame.cursor.1 - dy;
    }

    for ((name, modules), w) in windows.iter().zip(widths) {
        let Some(mut pos) = gui.windows.get(name).copied() else {
            continue;
        };
        // Header: `WMeteorHeader`, the accent color, title centered and the
        // triangle at the right.
        ui.rect(pos.x, pos.y, w, header, ACCENT);
        let tw = ui.text_w(name);
        ui.text(name, pos.x + (w - tw) / 2.0, pos.y + ui.theme.s(4.0), TEXT);
        let tri = ui.theme.text_h * 0.6;
        let tri_x = pos.x + w - tri - ui.theme.s(4.0);
        ui.triangle(tri_x, pos.y + (header - tri) / 2.0, tri, pos.expanded, TEXT);
        if ui.hovered(pos.x, pos.y, w, header) {
            if ui.frame.left_pressed {
                if ui.hovered(tri_x - ui.theme.s(4.0), pos.y, tri + ui.theme.s(8.0), header) {
                    pos.expanded = !pos.expanded;
                    meteor.mark_dirty();
                } else {
                    gui.drag = Some((name.clone(), ui.frame.cursor.0 - pos.x, ui.frame.cursor.1 - pos.y));
                    meteor.mark_dirty();
                }
            } else if ui.frame.right_pressed {
                pos.expanded = !pos.expanded;
                meteor.mark_dirty();
            }
        }
        if pos.expanded {
            let body_y = pos.y + header;
            let full = row * modules.len() as f32;
            let max_h = (ui.frame.sh - body_y - ui.theme.s(4.0)).max(row);
            let body_h = full.min(max_h);
            if ui.hovered(pos.x, body_y, w, body_h) && ui.frame.scroll != 0.0 {
                pos.scroll = (pos.scroll - ui.frame.scroll * row).clamp(0.0, (full - body_h).max(0.0));
            }
            ui.rect(pos.x, body_y, w, body_h, BACKGROUND[0]);
            ui.elements.push(MenuElement::ScissorPush {
                x: pos.x,
                y: body_y,
                w,
                h: body_h,
            });
            for (n, &index) in modules.iter().enumerate() {
                let y = body_y + n as f32 * row - pos.scroll;
                if y + row < body_y || y > body_y + body_h {
                    continue;
                }
                module_row(ui, gui, meteor, index, pos.x, y, w, body_y, body_h);
            }
            ui.elements.push(MenuElement::ScissorPop);
        }
        gui.windows.insert(name.clone(), pos);
    }

    // `ModulesScreen` help, bottom left.
    let pad = ui.theme.s(4.0);
    let line = ui.theme.text_h + ui.theme.s(2.0);
    let y = ui.frame.sh - pad - line * 2.0;
    ui.text("Left click - Toggle module", pad, y, TEXT);
    ui.text("Right click - Open module settings", pad, y + line, TEXT);

    // `WMeteorTooltip`: the hovered module's description.
    if gui.drag.is_none()
        && let Some(index) = hovered_module(ui, gui, &windows, row, header)
    {
        tooltip(ui, &CATALOG.modules[index].description);
    }
}

fn hovered_module(ui: &Ui, gui: &GuiState, windows: &[(String, Vec<usize>)], row: f32, header: f32) -> Option<usize> {
    let (cx, cy) = ui.frame.cursor;
    for (name, modules) in windows {
        let pos = gui.windows.get(name)?;
        if !pos.expanded || cx < pos.x {
            continue;
        }
        let rel = cy - pos.y - header + pos.scroll;
        if rel < 0.0 {
            continue;
        }
        let n = (rel / row) as usize;
        let width = modules
            .iter()
            .map(|&i| ui.text_w(&CATALOG.modules[i].title))
            .fold(0.0, f32::max)
            + ui.theme.s(8.0);
        if n < modules.len() && cx < pos.x + width.max(ui.text_w(name) + ui.theme.s(16.0) + ui.theme.text_h) {
            return Some(modules[n]);
        }
    }
    None
}

fn tooltip(ui: &mut Ui, text: &str) {
    let pad = ui.theme.s(4.0);
    let (cx, cy) = ui.frame.cursor;
    let w = ui.text_w(text) + pad * 2.0;
    let h = ui.theme.text_h + pad * 2.0;
    let x = (cx + ui.theme.s(12.0)).min(ui.frame.sw - w);
    let y = (cy + ui.theme.s(12.0)).min(ui.frame.sh - h);
    ui.rect(x, y, w, h, BACKGROUND[0]);
    ui.text(text, x + pad, y + pad, TEXT);
}

/// `WMeteorModule`: the background while active or hovered, the accent bar
/// at the left while active, the title centered.
#[allow(clippy::too_many_arguments)]
fn module_row(
    ui: &mut Ui,
    gui: &mut GuiState,
    meteor: &mut MeteorClient,
    index: usize,
    x: f32,
    y: f32,
    w: f32,
    clip_y: f32,
    clip_h: f32,
) {
    let row = row_h(&ui.theme);
    let active = meteor.modules[index].active;
    let hovered = ui.hovered(x, y, w, row) && ui.frame.cursor.1 >= clip_y && ui.frame.cursor.1 < clip_y + clip_h;
    if active || hovered {
        ui.rect(x, y, w, row, MODULE_BACKGROUND);
    }
    if active {
        ui.rect(x, y, ui.theme.s(2.0), row, ACCENT);
    }
    let title = &CATALOG.modules[index].title;
    let tw = ui.text_w(title);
    ui.text(title, x + (w - tw) / 2.0, y + ui.theme.s(4.0), TEXT);
    if hovered && gui.drag.is_none() {
        if ui.frame.left_pressed {
            meteor.toggle(index);
        } else if ui.frame.right_pressed {
            gui.screen = Screen::Module(index);
            gui.settings_scroll = 0.0;
        }
    }
}

/// `ModuleScreen`: the module's title and description, its settings by
/// group, then its bind and toggles.
fn module_screen(ui: &mut Ui, gui: &mut GuiState, meteor: &mut MeteorClient, index: usize) {
    let info = &CATALOG.modules[index];
    let pad = ui.theme.s(6.0);
    let row = ui.theme.text_h + ui.theme.s(10.0);
    let header = ui.theme.text_h + ui.theme.s(8.0);
    let label_w = info
        .groups
        .iter()
        .flat_map(|g| &g.settings)
        .map(|s| ui.text_w(&s.title))
        .fold(ui.text_w("Toggle on bind release"), f32::max);
    let widget_w = ui.theme.s(200.0);
    let w = (label_w + widget_w + pad * 4.0 + ui.theme.text_h)
        .max(ui.text_w(&info.description) + pad * 2.0)
        .min(ui.frame.sw - pad * 2.0);
    let rows = info.groups.iter().map(|g| 1 + g.settings.len()).sum::<usize>() + 6;
    let full = row * rows as f32 + pad * 2.0;
    let x = (ui.frame.sw - w) / 2.0;
    let top = ui.theme.text_h + ui.theme.s(12.0) + pad;
    let body_h = full.min(ui.frame.sh - top - header - pad);
    let y0 = top;

    ui.rect(x, y0, w, header, ACCENT);
    let tw = ui.text_w(&info.title);
    ui.text(&info.title, x + (w - tw) / 2.0, y0 + ui.theme.s(4.0), TEXT);
    let body_y = y0 + header;
    ui.rect(x, body_y, w, body_h, BACKGROUND[0]);
    if ui.hovered(x, body_y, w, body_h) && ui.frame.scroll != 0.0 {
        gui.settings_scroll = (gui.settings_scroll - ui.frame.scroll * row).clamp(0.0, (full - body_h).max(0.0));
    }
    ui.elements.push(MenuElement::ScissorPush {
        x,
        y: body_y,
        w,
        h: body_h,
    });
    let mut y = body_y + pad - gui.settings_scroll;
    ui.text(&info.description, x + pad, y, TEXT_SECONDARY);
    y += row;
    if !super::PORTED.contains(&info.name.as_str()) {
        ui.text("Behavior not ported yet; settings save.", x + pad, y, TEXT_SECONDARY);
        y += row;
    }

    let widget_x = x + w - pad - widget_w - ui.theme.text_h;
    let mut tooltip_text = None;
    for group in &info.groups {
        // `WMeteorSection`: the group's name between separator lines.
        let gw = ui.text_w(&group.name);
        let mid = x + w / 2.0;
        ui.rect(x + pad, y + ui.theme.text_h / 2.0, (mid - gw / 2.0 - pad * 2.0 - x).max(0.0), ui.theme.s(1.0), SEPARATOR_EDGES);
        ui.text(&group.name, mid - gw / 2.0, y, TEXT);
        ui.rect(mid + gw / 2.0 + pad, y + ui.theme.text_h / 2.0, (x + w - pad - (mid + gw / 2.0 + pad)).max(0.0), ui.theme.s(1.0), SEPARATOR_EDGES);
        y += row;
        for setting in &group.settings {
            let value = meteor.value(index, &group.name, &setting.name);
            let changed = meteor.modules[index]
                .values
                .contains_key(&(group.name.clone(), setting.name.clone()));
            ui.text(&setting.title, x + pad, y + ui.theme.s(2.0), TEXT);
            if ui.hovered(x + pad, y, label_w, row) {
                tooltip_text = Some(setting.description.as_str());
            }
            if let Some(new) = setting_widget(ui, gui, index, &group.name, setting, &value, widget_x, y, widget_w) {
                meteor.set_value(index, &group.name, &setting.name, new);
            }
            // Meteor's reset button, shown once a setting has changed.
            if changed {
                let rx = x + w - pad - ui.theme.text_h;
                let state = ui.state(rx, y, ui.theme.text_h, ui.theme.text_h + ui.theme.s(4.0));
                ui.text("R", rx + ui.theme.s(2.0), y + ui.theme.s(2.0), if state > 0 { MINUS } else { TEXT_SECONDARY });
                if state > 0 && ui.frame.left_pressed {
                    meteor.reset_value(index, &group.name, &setting.name);
                }
            }
            y += row;
        }
    }

    // Bind, toggle on bind release, chat feedback, active, favorite.
    ui.rect(x + pad, y + ui.theme.text_h / 2.0, w - pad * 2.0, ui.theme.s(1.0), SEPARATOR_EDGES);
    y += row;
    ui.text("Bind", x + pad, y + ui.theme.s(2.0), TEXT);
    let bind_label = if gui.binding == Some(index) {
        "Press a key...".to_owned()
    } else {
        meteor.modules[index].bind.as_deref().map_or("None".to_owned(), key_title)
    };
    if ui.button(&bind_label, widget_x, y, widget_w) {
        gui.binding = Some(index);
    }
    y += row;
    for (label, which) in [("Toggle on bind release", 0), ("Chat Feedback", 1), ("Active", 2), ("Favorite", 3)] {
        ui.text(label, x + pad, y + ui.theme.s(2.0), if which == 3 && meteor.modules[index].favorite { FAVORITE } else { TEXT });
        let state = &meteor.modules[index];
        let checked = [state.toggle_on_release, state.chat_feedback, state.active, state.favorite][which];
        if ui.checkbox(widget_x, y, checked) {
            match which {
                0 => meteor.modules[index].toggle_on_release = !checked,
                1 => meteor.modules[index].chat_feedback = !checked,
                2 => meteor.toggle(index),
                _ => meteor.modules[index].favorite = !checked,
            }
            meteor.mark_dirty();
        }
        y += row;
    }
    ui.elements.push(MenuElement::ScissorPop);
    if let Some(text) = tooltip_text.filter(|t| !t.is_empty()) {
        tooltip(ui, text);
    }
}

/// The widget for one setting; a changed value when the player set one.
#[allow(clippy::too_many_arguments)]
fn setting_widget(
    ui: &mut Ui,
    gui: &mut GuiState,
    index: usize,
    group: &str,
    setting: &SettingInfo,
    value: &Value,
    x: f32,
    y: f32,
    w: f32,
) -> Option<Value> {
    match setting.kind.as_str() {
        "Bool" => {
            let checked = value.as_bool().unwrap_or(false);
            ui.checkbox(x, y, checked).then(|| Value::Bool(!checked))
        }
        "Int" | "Long" | "Double" => slider(ui, gui, index, group, setting, value, x, y, w),
        "Enum" => {
            let current = value.as_str().unwrap_or("");
            // `WDropdown`; left click picks the next value, right the previous.
            let label = current.replace('_', " ");
            let clicked = ui.button(&label, x, y, w);
            let pad = ui.theme.s(4.0);
            let right = ui.hovered(x, y, w, ui.theme.text_h + pad * 2.0) && ui.frame.right_pressed;
            let options = &setting.options;
            let at = options.iter().position(|o| o == current).unwrap_or(0);
            if (clicked || right) && !options.is_empty() {
                let next = if clicked { (at + 1) % options.len() } else { (at + options.len() - 1) % options.len() };
                return Some(Value::String(options[next].clone()));
            }
            None
        }
        "Color" => {
            let c = value.as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(255.0) as f32 / 255.0).collect::<Vec<_>>()).unwrap_or_default();
            let color = if c.len() >= 4 { [c[0], c[1], c[2], c[3]] } else { TEXT };
            let size = ui.theme.text_h + ui.theme.s(4.0);
            ui.rect(x, y, size, size, OUTLINE[0]);
            let b = ui.theme.s(1.0);
            ui.rect(x + b, y + b, size - b * 2.0, size - b * 2.0, color);
            ui.text(&format!("{}, {}, {}, {}", (color[0] * 255.0) as u8, (color[1] * 255.0) as u8, (color[2] * 255.0) as u8, (color[3] * 255.0) as u8), x + size + ui.theme.s(6.0), y + ui.theme.s(2.0), TEXT_SECONDARY);
            None
        }
        "Keybind" => {
            let label = value.as_str().map_or("None".to_owned(), key_title);
            ui.button(&label, x, y, w);
            None
        }
        "String" => {
            ui.text(value.as_str().unwrap_or(""), x, y + ui.theme.s(2.0), TEXT_SECONDARY);
            None
        }
        _ => {
            let count = value.as_array().map_or(0, Vec::len);
            ui.text(&format!("{count} selected"), x, y + ui.theme.s(2.0), TEXT_SECONDARY);
            None
        }
    }
}

/// `WSlider` with its value: the left part filled to the handle.
#[allow(clippy::too_many_arguments)]
fn slider(
    ui: &mut Ui,
    gui: &mut GuiState,
    index: usize,
    group: &str,
    setting: &SettingInfo,
    value: &Value,
    x: f32,
    y: f32,
    w: f32,
) -> Option<Value> {
    let int = setting.kind != "Double";
    let v = value.as_f64().unwrap_or(0.0);
    let (lo, hi) = setting.slider_range();
    let label = if int { format!("{}", v.round() as i64) } else { format!("{v:.2}") };
    let label_w = ui.text_w("0000.00");
    ui.text(&label, x, y + ui.theme.s(2.0), TEXT);
    let sx = x + label_w + ui.theme.s(6.0);
    let sw = (w - label_w - ui.theme.s(6.0)).max(ui.theme.s(20.0));
    let h = ui.theme.text_h + ui.theme.s(4.0);
    let bar_h = ui.theme.s(3.0);
    let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let handle = ui.theme.text_h * 0.8;
    let hx = sx + (sw - handle) * t;
    ui.rect(sx, y + (h - bar_h) / 2.0, hx - sx + handle / 2.0, bar_h, SLIDER_LEFT);
    ui.rect(hx + handle / 2.0, y + (h - bar_h) / 2.0, sx + sw - hx - handle / 2.0, bar_h, SLIDER_RIGHT);
    let state = ui.state(sx, y, sw, h);
    ui.elements.push(MenuElement::Rect {
        x: hx,
        y: y + (h - handle) / 2.0,
        w: handle,
        h: handle,
        corner_radius: handle / 2.0,
        color: SLIDER_HANDLE[state],
    });
    let key = (index, group.to_owned(), setting.name.clone());
    if state > 0 && ui.frame.left_pressed {
        gui.slider = Some(key.clone());
    }
    if gui.slider.as_ref() == Some(&key) {
        let t = ((ui.frame.cursor.0 - sx - handle / 2.0) / (sw - handle)).clamp(0.0, 1.0) as f64;
        let mut new = setting.clamp(lo + (hi - lo) * t);
        if int {
            new = new.round();
        } else {
            new = (new * 100.0).round() / 100.0;
        }
        if (new - v).abs() > f64::EPSILON {
            return Some(if int { json!(new as i64) } else { json!(new) });
        }
    }
    None
}

/// A winit key name as Meteor shows binds: `KeyR` as `R`.
fn key_title(name: &str) -> String {
    name.strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let mut out = String::new();
            for (i, c) in name.chars().enumerate() {
                if i > 0 && c.is_uppercase() {
                    out.push(' ');
                }
                out.push(c);
            }
            out
        })
}

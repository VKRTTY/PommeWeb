//! The page's loading screen, which stays over the canvas until the game is
//! ready: the splash's progress goes to it, and it lifts at the title menu.

use wasm_bindgen::{JsCast, JsValue};

fn call(name: &str, args: &[JsValue]) {
    let Ok(function) = js_sys::Reflect::get(&js_sys::global(), &name.into()) else {
        return;
    };
    let Some(function) = function.dyn_ref::<js_sys::Function>() else {
        return;
    };
    let args: js_sys::Array = args.iter().collect();
    let _ = function.apply(&JsValue::NULL, &args);
}

/// What the splash shows: `progress` from 0 to 1, and Pomme's status.
pub fn splash(progress: f32, status: &str) {
    call("pommeSplash", &[progress.into(), status.into()]);
}

/// The game is at its title menu: the page's loading screen goes.
pub fn ready() {
    call("pommeReady", &[]);
}

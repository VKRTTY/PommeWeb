//! The browser's side of `pomme_dirs::fs`: the page's IndexedDB store
//! fills the in-memory file tree before the game starts, takes each change
//! the game makes, and serves asset objects (mostly sounds) when first read.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use js_sys::{Array, Promise, Uint8Array};
use pomme_dirs::fs::Change;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[wasm_bindgen(raw_module = "/asset-storage.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = assetPreload, catch)]
    fn asset_preload() -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = assetRemove, catch)]
    fn asset_remove(path: &str) -> Result<Promise, JsValue>;
}

thread_local! {
    static FETCHING: RefCell<HashSet<PathBuf>> = RefCell::default();
}

/// Loads the stored files into the tree and starts keeping the store
/// current.
pub async fn init() -> Result<(), JsValue> {
    let entries: Array = JsFuture::from(asset_preload()?).await?.dyn_into()?;
    let mut bytes_total = 0usize;
    for pair in entries.to_vec().chunks_exact(2) {
        let (Some(path), Ok(bytes)) = (pair[0].as_string(), pair[1].clone().dyn_into::<Uint8Array>())
        else {
            continue;
        };
        let bytes = bytes.to_vec();
        bytes_total += bytes.len();
        pomme_dirs::fs::preload(path, bytes);
    }
    tracing::info!(
        "Loaded {} stored files ({:.1} MB)",
        entries.length() / 2,
        bytes_total as f64 / 1048576.0
    );
    pomme_dirs::fs::set_persist(persist);
    pomme_dirs::fs::set_fetch(fetch);
    Ok(())
}

fn persist(change: Change<'_>) {
    match change {
        Change::Write(path, bytes) => {
            let (path, bytes) = (path.to_path_buf(), bytes.to_vec());
            spawn_local(async move {
                if let Err(e) = pomme_install::write_atomic(&path, &bytes).await {
                    tracing::warn!("Couldn't store {}: {e}", path.display());
                }
            });
        }
        Change::Remove(path) => {
            if let Ok(promise) = asset_remove(&path.to_string_lossy()) {
                spawn_local(async move {
                    let _ = JsFuture::from(promise).await;
                });
            }
        }
    }
}

/// Asset objects load in the background when first read; the read before
/// they arrive finds them busy.
fn fetch(path: &Path) -> bool {
    if !pomme_install::web::is_object(&path.to_string_lossy()) {
        return false;
    }
    let path = path.to_path_buf();
    if !FETCHING.with_borrow_mut(|f| f.insert(path.clone())) {
        return true;
    }
    spawn_local(async move {
        match pomme_install::web::fetch_object(crate::net::http::client(), &path).await {
            Ok(bytes) => pomme_dirs::fs::preload(&path, bytes),
            Err(e) => tracing::warn!("Couldn't load {}: {e}", path.display()),
        }
        FETCHING.with_borrow_mut(|f| f.remove(&path));
    });
    true
}

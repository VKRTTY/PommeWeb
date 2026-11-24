//! IndexedDB storage for the browser's game assets.
use std::path::Path;

use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(raw_module = "/asset-storage.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = assetRead, catch)]
    fn read(path: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = assetHas, catch)]
    fn has(path: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = assetWrite, catch)]
    fn write(path: &str, bytes: &Uint8Array) -> Result<Promise, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = setTimeout)]
    fn set_timeout(callback: &js_sys::Function, delay_ms: i32) -> i32;
}

fn reason(error: JsValue) -> String {
    format!("Browser asset storage: {error:?}")
}

pub async fn read_asset(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = path.to_string_lossy();
    let value = JsFuture::from(read(&path).map_err(reason)?).await.map_err(reason)?;
    if value.is_undefined() {
        return Ok(None);
    }
    let bytes = value.dyn_into::<Uint8Array>().map_err(reason)?;
    Ok(Some(bytes.to_vec()))
}

pub(super) async fn exists(path: &Path) -> Result<bool, String> {
    let path = path.to_string_lossy();
    let value = JsFuture::from(has(&path).map_err(reason)?).await.map_err(reason)?;
    Ok(value.as_bool().unwrap_or(false))
}

pub(super) async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let path = path.to_string_lossy();
    JsFuture::from(write(&path, &Uint8Array::from(bytes)).map_err(reason)?)
        .await
        .map_err(reason)?;
    Ok(())
}

pub(super) async fn sleep(time: std::time::Duration) {
    let promise = Promise::new(&mut |resolve, _| {
        set_timeout(&resolve, time.as_millis().min(i32::MAX as u128) as i32);
    });
    let _ = JsFuture::from(promise).await;
}

pub(super) fn relay_url(url: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    let host = url.host_str().ok_or("Asset URL has no host")?;
    if url.scheme() != "https" || url.port().is_some() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some()
    {
        return Err("Invalid upstream asset URL".into());
    }
    let origin = js_sys::Reflect::get(&js_sys::global(), &"origin".into())
        .map_err(reason)?
        .as_string()
        .ok_or("Missing browser origin")?;
    let query = url.query().map(|query| format!("?{query}")).unwrap_or_default();
    Ok(format!("{origin}/relay/{host}{}{query}", url.path()))
}

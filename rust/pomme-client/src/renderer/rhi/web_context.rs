//! Device/canvas bootstrap shared by the browser renderer and its swapchain.
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(raw_module = "/gpu-context.mjs")]
extern "C" {
    pub type BrowserGpuContext;

    #[wasm_bindgen(static_method_of = BrowserGpuContext, catch, js_name = create)]
    fn create_raw(canvas: &JsValue) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, getter)]
    pub fn device(this: &BrowserGpuContext) -> JsValue;

    #[wasm_bindgen(method, getter)]
    pub fn format(this: &BrowserGpuContext) -> String;

    #[wasm_bindgen(method, catch)]
    pub fn resize(this: &BrowserGpuContext, width: u32, height: u32) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn acquire(this: &BrowserGpuContext) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = shader)]
    fn shader_raw(this: &BrowserGpuContext, label: &str, source: &str)
        -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch, js_name = createBuffer)]
    pub(super) fn create_buffer_raw(this: &BrowserGpuContext, label: &str, size: f64, flags: u8, memory: u8) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = writeBuffer)]
    pub(super) fn write_buffer_raw(this: &BrowserGpuContext, buffer: &JsValue, offset: f64, bytes: &js_sys::Uint8Array) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = createTexture)]
    pub(super) fn create_texture_raw(this: &BrowserGpuContext, label: &str, width: u32, height: u32, layers: u32, levels: u32, format: &str, flags: u8) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = createTextureView)]
    pub(super) fn create_view_raw(this: &BrowserGpuContext, texture: &JsValue, dimension: &str, first_mip: u32, levels: u32, first_layer: u32, layers: u32) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = createSampler)]
    pub(super) fn create_sampler_raw(this: &BrowserGpuContext, filter: &str, address: &str, mipmap: &str, max_lod: f32, compare: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = createRenderPipeline)]
    pub(super) fn create_render_pipeline_raw(
        this: &BrowserGpuContext, json: &str, vertex: &JsValue, fragment: &JsValue, layout: &JsValue,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method)]
    pub fn destroy(this: &BrowserGpuContext);
}

impl BrowserGpuContext {
    pub async fn new(canvas: &JsValue) -> Result<Self, JsValue> {
        JsFuture::from(Self::create_raw(canvas)?)
            .await?
            .dyn_into()
    }

    pub async fn load_embedded_shader(
        &self, label: &str, source: super::Shader<'_>,
    ) -> Result<JsValue, JsValue> {
        let source = std::str::from_utf8(source)
            .map_err(|e| JsValue::from_str(&format!("{label}: invalid WGSL: {e}")))?;
        self.load_shader(label, source).await
    }

    pub async fn load_shader(&self, label: &str, source: &str) -> Result<JsValue, JsValue> {
        JsFuture::from(self.shader_raw(label, source)?).await
    }
}

//! Browser command submission using the same attachment operations and
//! framebuffer coordinates as Pomme's native renderer.
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

use super::web_context::BrowserGpuContext;
use super::{BufferCopy, LoadOp, Rect, RenderPassDesc, StoreOp};

#[wasm_bindgen(raw_module = "/gpu-context.mjs")]
extern "C" {
    pub type BrowserCommands;
    pub type BrowserPass;

    #[wasm_bindgen(method, catch, js_name = beginCommands)]
    pub fn begin_commands(this: &BrowserGpuContext, label: &str) -> Result<BrowserCommands, JsValue>;

    #[wasm_bindgen(method, catch, js_name = beginRenderPass)]
    fn begin_render_raw(
        this: &BrowserCommands,
        colors: &Array,
        depth: &JsValue,
        width: u32,
        height: u32,
    ) -> Result<BrowserPass, JsValue>;

    #[wasm_bindgen(method, catch, js_name = beginComputePass)]
    pub fn begin_compute(this: &BrowserCommands) -> Result<BrowserPass, JsValue>;

    #[wasm_bindgen(method, catch, js_name = copyBuffer)]
    fn copy_buffer_raw(this: &BrowserCommands, src: &JsValue, src_offset: f64, dst: &JsValue,
        dst_offset: f64, size: f64) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = copyTexturePixel)]
    pub fn copy_texture_pixel(this: &BrowserCommands, texture: &JsValue, x: u32, y: u32,
        readback: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn submit(this: &BrowserCommands) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn pipeline(this: &BrowserPass, pipeline: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = vertexBuffer)]
    fn vertex_buffer_raw(this: &BrowserPass, slot: u32, buffer: &JsValue, offset: f64) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = indexBuffer)]
    fn index_buffer_raw(this: &BrowserPass, buffer: &JsValue, format: &str, offset: f64) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn bind(this: &BrowserPass, slot: u32, group: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = scissor)]
    fn scissor_raw(this: &BrowserPass, x: u32, y: u32, width: u32, height: u32) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn draw(this: &BrowserPass, vertices: u32, instances: u32, first_vertex: u32,
        first_instance: u32) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = drawIndexed)]
    pub fn draw_indexed(this: &BrowserPass, indices: u32, instances: u32, first_index: u32,
        base_vertex: i32, first_instance: u32) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = drawIndexedIndirect)]
    fn draw_indexed_indirect_raw(this: &BrowserPass, buffer: &JsValue, offset: f64) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn dispatch(this: &BrowserPass, x: u32, y: u32, z: u32) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    pub fn end(this: &BrowserPass) -> Result<(), JsValue>;
}

impl BrowserCommands {
    pub fn begin_render<Format>(
        &self,
        desc: &RenderPassDesc<&JsValue, Format>,
    ) -> Result<BrowserPass, JsValue> {
        let colors = Array::new();
        if desc.color.is_none() && !desc.extra_colors.is_empty() {
            colors.push(&JsValue::NULL);
        }
        for attachment in desc.color.iter().chain(&desc.extra_colors) {
            let object = Object::new();
            field(&object, "view", attachment.texture)?;
            field(&object, "storeOp", &store(attachment.store).into())?;
            match attachment.load {
                LoadOp::Load => field(&object, "loadOp", &"load".into())?,
                LoadOp::Clear(rgba) => {
                    field(&object, "loadOp", &"clear".into())?;
                    let value: Array = rgba.into_iter().map(JsValue::from).collect();
                    field(&object, "clearValue", &value)?;
                }
            }
            colors.push(&object);
        }

        let depth = if let Some(attachment) = &desc.depth {
            let object = Object::new();
            field(&object, "view", attachment.texture)?;
            field(&object, "depthStoreOp", &store(attachment.store).into())?;
            match attachment.load {
                LoadOp::Load => field(&object, "depthLoadOp", &"load".into())?,
                LoadOp::Clear(value) => {
                    field(&object, "depthLoadOp", &"clear".into())?;
                    field(&object, "depthClearValue", &value.into())?;
                }
            }
            object.into()
        } else {
            JsValue::NULL
        };
        self.begin_render_raw(&colors, &depth, desc.extent.width, desc.extent.height)
    }

    pub fn copy_buffer(&self, src: &JsValue, dst: &JsValue, copy: BufferCopy) -> Result<(), JsValue> {
        self.copy_buffer_raw(src, number(copy.src_offset)?, dst, number(copy.dst_offset)?, number(copy.size)?)
    }
}

impl BrowserPass {
    pub fn vertex_buffer(&self, slot: u32, buffer: &JsValue, offset: u64) -> Result<(), JsValue> {
        self.vertex_buffer_raw(slot, buffer, number(offset)?)
    }

    pub fn index_buffer(&self, buffer: &JsValue, wide: bool, offset: u64) -> Result<(), JsValue> {
        self.index_buffer_raw(buffer, if wide { "uint32" } else { "uint16" }, number(offset)?)
    }

    pub fn scissor(&self, rect: Rect) -> Result<(), JsValue> {
        self.scissor_raw(rect.x, rect.y, rect.width, rect.height)
    }

    pub fn draw_indexed_indirect(&self, buffer: &JsValue, offset: u64) -> Result<(), JsValue> {
        self.draw_indexed_indirect_raw(buffer, number(offset)?)
    }
}

fn field(object: &Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
    Reflect::set(object, &key.into(), value).map(|_| ())
}

fn store(op: StoreOp) -> &'static str {
    match op {
        StoreOp::Store => "store",
        StoreOp::Discard => "discard",
    }
}

fn number(value: u64) -> Result<f64, JsValue> {
    if value >= 1 << 53 {
        return Err(JsValue::from_str("WebGPU command argument exceeds JavaScript integer precision"));
    }
    Ok(value as f64)
}

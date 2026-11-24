//! The WebGPU backend of [`super`], for the browser build. GPU objects live
//! in the page's `webgpu.mjs`; Rust holds them as integer ids, which keeps
//! the handle types `Copy` as the Vulkan backend's are.
//!
//! WebGPU has no push constants: the shaders read them from a uniform that
//! `webgpu.mjs` fills per draw, bound at the group after a pipeline's own.
//! Bind group layouts are made lazily, from what the shaders and the bound
//! resources say each binding is, since WebGPU's layouts name more than the
//! RHI's do (texture dimensions, read-only storage, sample types).

use std::cell::RefCell;
use std::collections::HashMap;

use js_sys::Array;
use serde_json::{Value, json};
use thiserror::Error;
use wasm_bindgen::prelude::*;
use winit::window::Window;

use super::{
    Acquired, AddressMode, BindGroupLayoutEntry, BindingType, BlendComponent, BlendFactor, Blit,
    BufferCopy, BufferUsages, BufferUse, CompareFunction, CullMode, Extent, FilterMode, FrontFace,
    IndirectDrawFeatures, LoadOp, MemoryLocation, Presented, PushConstants, Rect,
    RenderPipelineDesc, SamplerDesc, ShaderStages, StoreOp, TexelCopy, TextureDesc, TextureFormat,
    TextureRange, TextureUsages, TextureViewDimension, Topology, VertexFormat, VertexStepMode,
};
use crate::renderer::rhi;

#[wasm_bindgen(raw_module = "/webgpu.mjs")]
extern "C" {
    #[wasm_bindgen(catch, js_name = init)]
    async fn js_init(canvas: &JsValue) -> Result<JsValue, JsValue>;
    fn limit(name: &str) -> f64;
    fn configure(width: u32, height: u32) -> String;
    fn acquire();
    #[wasm_bindgen(js_name = destroy)]
    fn js_destroy(id: u32);

    #[wasm_bindgen(js_name = createBuffer)]
    fn create_buffer(label: &str, size: f64, usage: u8, readback: bool) -> u32;
    #[wasm_bindgen(js_name = writeBuffer)]
    fn write_buffer(id: u32, offset: f64, bytes: &[u8]);
    #[wasm_bindgen(js_name = readBuffer)]
    fn read_buffer(id: u32) -> JsValue;

    #[wasm_bindgen(js_name = createTexture)]
    fn create_texture(
        label: &str,
        width: u32,
        height: u32,
        layers: u32,
        mips: u32,
        format: &str,
        renderable: bool,
    ) -> u32;
    #[wasm_bindgen(js_name = createView)]
    fn create_view(
        texture: u32,
        dimension: &str,
        first_mip: u32,
        mips: u32,
        first_layer: u32,
        layers: u32,
    ) -> u32;
    #[wasm_bindgen(js_name = createSampler)]
    fn create_sampler_js(filter: &str, address: &str, mipmap: &str, max_lod: f32, compare: &str)
    -> u32;
    #[wasm_bindgen(js_name = writeTexture)]
    fn write_texture(
        texture: u32,
        mip: u32,
        layer: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        bytes_per_row: u32,
        bytes: &[u8],
    );

    #[wasm_bindgen(js_name = createBindGroupLayout)]
    fn create_bind_group_layout_js(json: &str) -> u32;
    #[wasm_bindgen(js_name = createPipelineLayout)]
    fn create_pipeline_layout_js(groups: &[u32], push_group: i32) -> u32;
    #[wasm_bindgen(js_name = createShaderModule)]
    fn create_shader_module(label: &str, code: &str) -> u32;
    #[wasm_bindgen(js_name = createRenderPipeline)]
    fn create_render_pipeline_js(json: &str, layout: u32, vertex: u32, fragment: u32) -> u32;
    #[wasm_bindgen(js_name = createComputePipeline)]
    fn create_compute_pipeline_js(label: &str, layout: u32, module: u32) -> u32;
    #[wasm_bindgen(js_name = createBindGroup)]
    fn create_bind_group_js(layout: u32, entries: &[f64]) -> u32;

    #[wasm_bindgen(js_name = beginEncoder)]
    fn begin_encoder(label: &str) -> u32;
    #[wasm_bindgen(js_name = submit)]
    fn submit_js(encoder: u32, frame: bool);
    #[wasm_bindgen(js_name = copyBuffer)]
    fn copy_buffer_js(encoder: u32, src: u32, src_offset: f64, dst: u32, dst_offset: f64, size: f64);
    #[wasm_bindgen(js_name = clearBuffer)]
    fn clear_buffer_js(encoder: u32, buffer: u32, offset: f64, size: f64, value: u32);
    #[wasm_bindgen(js_name = copyBufferToTexture)]
    fn copy_buffer_to_texture_js(
        encoder: u32,
        buffer: u32,
        offset: f64,
        bytes_per_row: u32,
        texture: u32,
        mip: u32,
        layer: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    );
    #[wasm_bindgen(js_name = copyTextureToBuffer)]
    fn copy_texture_to_buffer_js(
        encoder: u32,
        texture: u32,
        mip: u32,
        layer: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        buffer: u32,
        offset: f64,
        bytes_per_row: u32,
    );
    #[wasm_bindgen(js_name = copyTexture)]
    fn copy_texture_js(encoder: u32, src: u32, dst: u32, width: u32, height: u32);
    #[wasm_bindgen(js_name = clearTexture)]
    fn clear_texture_js(
        encoder: u32,
        texture: u32,
        first_mip: u32,
        mips: u32,
        first_layer: u32,
        layers: u32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );
    #[wasm_bindgen(js_name = blit)]
    fn blit_js(encoder: u32, src: u32, dst: u32, src_level: u32, dst_level: u32, layers: u32);
    #[wasm_bindgen(js_name = dispatch)]
    fn dispatch_js(
        encoder: u32,
        pipeline: u32,
        group: u32,
        push: &[u8],
        x: u32,
        y: u32,
        z: u32,
        indirect: u32,
        indirect_offset: f64,
    );

    #[wasm_bindgen(js_name = beginRenderPass)]
    fn begin_render_pass_js(encoder: u32, colors: &Array, depth: &[f64], width: u32, height: u32)
    -> u32;
    #[wasm_bindgen(js_name = endPass)]
    fn end_pass(pass: u32);
    #[wasm_bindgen(js_name = setPipeline)]
    fn set_pipeline_js(pass: u32, pipeline: u32);
    #[wasm_bindgen(js_name = setBindGroup)]
    fn set_bind_group_js(pass: u32, index: u32, group: u32);
    #[wasm_bindgen(js_name = setPush)]
    fn set_push(pass: u32, offset: u32, bytes: &[u8]);
    #[wasm_bindgen(js_name = setVertexBuffer)]
    fn set_vertex_buffer_js(pass: u32, slot: u32, buffer: u32);
    #[wasm_bindgen(js_name = setIndexBuffer)]
    fn set_index_buffer_js(pass: u32, buffer: u32);
    #[wasm_bindgen(js_name = setViewport)]
    fn set_viewport_js(pass: u32, x: f32, y: f32, width: f32, height: f32);
    #[wasm_bindgen(js_name = setScissor)]
    fn set_scissor_js(pass: u32, x: u32, y: u32, width: u32, height: u32);
    #[wasm_bindgen(js_name = draw)]
    fn draw_js(pass: u32, vertices: u32, instances: u32, first_vertex: u32, first_instance: u32);
    #[wasm_bindgen(js_name = drawIndexed)]
    fn draw_indexed_js(
        pass: u32,
        indices: u32,
        instances: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    );
    #[wasm_bindgen(js_name = drawIndexedIndirect)]
    fn draw_indexed_indirect_js(pass: u32, buffer: u32, offset: f64, count: u32, stride: u32);
    #[wasm_bindgen(js_name = clearRect)]
    fn clear_rect(
        pass: u32,
        depth: bool,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    );
}

/// The canvas texture's view and texture, which `acquire` replaces.
const SWAPCHAIN_VIEW: u32 = 1;
const SWAPCHAIN_TEXTURE: u32 = 2;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct Error(String);

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("WebGPU: {0}")]
    WebGpu(String),
    #[error(transparent)]
    Gpu(#[from] Error),
}

/// A texture format as WebGPU names it; the surface's may be BGRA, which
/// isn't one of the RHI's [`TextureFormat`]s.
pub type Format = &'static str;

// ---- What Rust knows about the objects it made ----

struct ViewInfo {
    texture: u32,
    format: Format,
    dimension: &'static str,
}

/// A binding's kind from a shader's `// pomme-bindings` header: `u`, `sr`,
/// `sw`, `sf`, `sc`, or `t?` with a view dimension.
#[derive(Clone)]
struct Reflected {
    kind: String,
    dimension: Option<String>,
}

struct LayoutInfo {
    entries: Vec<BindGroupLayoutEntry>,
    reflected: Vec<Option<Reflected>>,
    /// Whether a bind group has held a depth texture at each binding.
    depth_seen: Vec<bool>,
}

struct PipelineLayoutInfo {
    groups: Vec<BindGroupLayout>,
    push: Option<PushConstants>,
}

#[derive(Default)]
struct Registry {
    /// Host buffers' contents, which writes of any alignment and
    /// buffer-to-texture copies of any row length are served from.
    shadows: HashMap<u32, Vec<u8>>,
    views: HashMap<u32, ViewInfo>,
    texture_formats: HashMap<u32, Format>,
    /// Whether each sampler compares.
    samplers: HashMap<u32, bool>,
    layouts: Vec<LayoutInfo>,
    pipeline_layouts: Vec<PipelineLayoutInfo>,
    js_layouts: HashMap<String, u32>,
    js_pipeline_layouts: HashMap<(Vec<u32>, i32), u32>,
    modules: HashMap<(usize, usize, i32), u32>,
    warned: Vec<&'static str>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::default();
    static ADAPTER: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    REGISTRY.with_borrow_mut(f)
}

fn warn_once(what: &'static str) {
    let first = registry(|r| {
        let first = !r.warned.contains(&what);
        if first {
            r.warned.push(what);
        }
        first
    });
    if first {
        tracing::warn!("WebGPU backend: {what}");
    }
}

/// Opens the GPU and the page's canvas. The browser hands over devices
/// asynchronously, so this runs before the game starts; [`Context::new`]
/// then takes what it opened.
pub async fn init(canvas: &JsValue) -> Result<(), String> {
    let name = js_init(canvas)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    ADAPTER.set(Some(name.as_string().unwrap_or_else(|| "WebGPU".into())));
    Ok(())
}

// ---- Context, device and swapchain ----

pub struct Context {
    pub device: Device,
    pub indirect_draw: IndirectDrawFeatures,
    pub present_wait: bool,
    pub gpu_name: String,
    pub api_version: String,
}

impl Context {
    pub fn new(_window: &Window) -> Result<Self, ContextError> {
        let gpu_name = ADAPTER
            .with_borrow(Clone::clone)
            .ok_or_else(|| ContextError::WebGpu("the GPU wasn't opened before the game".into()))?;
        Ok(Self {
            device: Device {
                discrete: true,
                max_texture_array_layers: limit("maxTextureArrayLayers") as u32,
                max_texture_size: limit("maxTextureDimension2D") as u32,
            },
            indirect_draw: IndirectDrawFeatures {
                count: false,
                multi_draw: false,
                fragment_stores: false,
            },
            present_wait: false,
            gpu_name,
            api_version: "WebGPU".into(),
        })
    }

    pub fn destroy_surface(&mut self) {}

    pub fn recreate_surface(&mut self, _window: &Window) -> Result<(), ContextError> {
        Ok(())
    }
}

/// Memory heaps' budget for this process and what it uses of them, in bytes.
#[derive(Clone, Copy, Debug)]
pub struct MemoryBudget {
    pub budget: u64,
    pub usage: u64,
}

pub struct Device {
    /// Uploads go through staging buffers, as to a discrete GPU's memory:
    /// host buffers keep a copy in the page's memory, so they stay small.
    pub discrete: bool,
    pub max_texture_array_layers: u32,
    pub max_texture_size: u32,
}

impl Device {
    /// Browsers don't say; a modest figure keeps the arenas sized for a
    /// page's memory.
    pub fn memory_bytes(&self, _location: MemoryLocation) -> u64 {
        1 << 30
    }

    pub fn memory_budget(&self, _location: MemoryLocation) -> Option<MemoryBudget> {
        None
    }

    pub fn wait_idle(&self) {}

    pub fn forget_targets(&self, _textures: &[TextureHandle]) {}

    /// Records commands and queues them after all earlier work, which
    /// WebGPU's queue order gives without waiting.
    pub fn submit(&self, record: impl FnOnce(&CommandEncoder)) {
        let encoder = CommandEncoder::begin("submit");
        record(&encoder);
        submit_js(encoder.id, false);
    }

    pub fn try_submit(&self, record: impl FnOnce(&CommandEncoder)) -> Result<(), String> {
        self.submit(record);
        Ok(())
    }

    pub fn create_bind_group(
        &self,
        layout: BindGroupLayout,
        bindings: &[(u32, BindingResource)],
    ) -> BindGroup {
        let actual: Vec<(u32, Actual)> = registry(|r| {
            bindings
                .iter()
                .map(|&(binding, resource)| {
                    let actual = match resource {
                        BindingResource::Texture(t) => r.views.get(&t.view).map_or(
                            Actual::Texture {
                                depth: false,
                                dimension: "2d",
                            },
                            |v| Actual::Texture {
                                depth: v.format == "depth32float",
                                dimension: v.dimension,
                            },
                        ),
                        BindingResource::Sampler(s) => Actual::Sampler {
                            compare: r.samplers.get(&s.0).copied().unwrap_or(false),
                        },
                        _ => Actual::Buffer,
                    };
                    (binding, actual)
                })
                .collect()
        });
        let js_layout = layout.resolve(&actual);
        let mut entries = Vec::with_capacity(bindings.len() * 4);
        for &(binding, resource) in bindings {
            let (kind, id, size) = match resource {
                BindingResource::UniformBuffer { buffer, size }
                | BindingResource::StorageBuffer { buffer, size } => (0.0, buffer.0, size as f64),
                BindingResource::Texture(texture) => (1.0, texture.view, 0.0),
                BindingResource::Sampler(sampler) => (2.0, sampler.0, 0.0),
            };
            entries.extend([binding as f64, kind, id as f64, size]);
        }
        BindGroup(create_bind_group_js(js_layout, &entries))
    }

    /// # Safety
    /// Nothing may use the device afterwards.
    pub unsafe fn destroy(&mut self) {}
}

pub struct Swapchain {
    pub format: Format,
    pub extent: Extent,
    pub depth: Texture,
}

impl Swapchain {
    pub fn new(_ctx: &Context, width: u32, height: u32, _vsync: bool) -> Result<Self, ContextError> {
        let format = match configure(width, height).as_str() {
            "bgra8unorm" => "bgra8unorm",
            _ => "rgba8unorm",
        };
        registry(|r| {
            r.views.insert(
                SWAPCHAIN_VIEW,
                ViewInfo {
                    texture: SWAPCHAIN_TEXTURE,
                    format,
                    dimension: "2d",
                },
            );
            r.texture_formats.insert(SWAPCHAIN_TEXTURE, format);
        });
        let extent = Extent { width, height };
        let depth = Texture::try_new(&DEVICE_STUB, &TextureDesc::swapchain_depth(extent))
            .map_err(ContextError::WebGpu)?;
        Ok(Self {
            format,
            extent,
            depth,
        })
    }

    pub fn rebuild(
        &mut self,
        ctx: &Context,
        _frames: &mut Frames,
        width: u32,
        height: u32,
        vsync: bool,
    ) -> Result<(), ContextError> {
        let new = Self::new(ctx, width, height, vsync)?;
        std::mem::replace(self, new).destroy(ctx);
        Ok(())
    }

    pub fn image_count(&self) -> usize {
        1
    }

    pub fn targets(&self) -> PipelineTargets {
        PipelineTargets::new(self.format, true)
    }

    pub fn image(&self, _index: u32) -> TextureHandle {
        TextureHandle {
            texture: SWAPCHAIN_TEXTURE,
            view: SWAPCHAIN_VIEW,
        }
    }

    pub fn pass(&self, index: u32, color: LoadOp<[f32; 4]>) -> RenderPassDesc {
        RenderPassDesc::swapchain(
            self.image(index),
            self.format,
            color,
            self.depth.handle(),
            self.extent,
        )
    }

    pub fn pass_keeping_depth(
        &self,
        index: u32,
        color: LoadOp<[f32; 4]>,
        depth: LoadOp<f32>,
    ) -> RenderPassDesc {
        RenderPassDesc {
            depth: Some(DepthAttachment {
                texture: self.depth.handle(),
                load: depth,
                store: StoreOp::Store,
            }),
            ..self.pass(index, color)
        }
    }

    pub fn is_fifo(&self) -> bool {
        true
    }

    pub fn present_mode(&self) -> Option<rhi::PresentMode> {
        Some(rhi::PresentMode::Fifo)
    }

    pub fn surface_resized(&self, _ctx: &Context) -> bool {
        false
    }

    pub fn is_bgra(&self) -> bool {
        self.format.starts_with("bgra")
    }

    pub fn ready_to_present(&self, _encoder: &CommandEncoder, _index: u32) {}

    pub fn release_surface(&mut self, _ctx: &Context) {}

    pub fn destroy(&mut self, ctx: &Context) {
        std::mem::take(&mut self.depth).destroy(&ctx.device);
    }
}

/// The few calls that take a device but need nothing from it.
const DEVICE_STUB: Device = Device {
    discrete: true,
    max_texture_array_layers: 0,
    max_texture_size: 0,
};

/// The canvas presents what was drawn when the page's frame callback
/// returns, so a frame is a command encoder and an acquired canvas texture.
pub struct Frames {
    index: usize,
}

impl Frames {
    pub fn new(_device: &Device, _image_count: usize) -> Result<Self, Error> {
        Ok(Self { index: 0 })
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn wait(&self, _device: &Device) -> Result<(), Error> {
        Ok(())
    }

    pub fn wait_all(&self, _device: &Device) {}

    pub fn acquire(&self, _device: &Device, _swapchain: &Swapchain) -> Result<Acquired, Error> {
        acquire();
        Ok(Acquired::Image {
            index: 0,
            suboptimal: false,
        })
    }

    pub fn begin(&self, _device: &Device) -> Result<CommandEncoder, Error> {
        Ok(CommandEncoder::begin("frame"))
    }

    pub fn submit_and_present(
        &mut self,
        encoder: CommandEncoder,
        _ctx: &Context,
        _swapchain: &Swapchain,
        _image: u32,
    ) -> Result<Presented, Error> {
        submit_js(encoder.id, true);
        self.index = (self.index + 1) % crate::renderer::MAX_FRAMES_IN_FLIGHT;
        Ok(Presented::Done)
    }

    pub fn pace(&self, _ctx: &Context, _swapchain: &Swapchain, _timeout_ns: u64) -> bool {
        true
    }

    pub fn resize(&mut self, _device: &Device, _image_count: usize) -> Result<(), Error> {
        Ok(())
    }

    pub fn destroy(&mut self, _device: &Device) {}
}

/// GPU timestamps aren't read back in the browser.
pub struct GpuTimer;

impl GpuTimer {
    pub fn new(_ctx: &Context, _per_frame: u32) -> Option<Self> {
        None
    }

    pub fn begin_frame(&mut self, _encoder: &CommandEncoder, _frame: usize) {}

    pub fn stamp(&mut self, _encoder: &CommandEncoder, _frame: usize, _what: &'static str) {}

    pub fn submitted(&mut self, _frame: usize) {}

    pub fn read(&self, _device: &Device, _frame: usize) -> Vec<(&'static str, f32)> {
        Vec::new()
    }

    pub fn destroy(&mut self, _device: &Device) {}
}

// ---- Buffers ----

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct BufferHandle(u32);

#[derive(Default)]
pub struct Buffer {
    id: u32,
    size: u64,
    readback: bool,
}

fn padded(size: u64) -> usize {
    (size.max(4).div_ceil(4) * 4) as usize
}

impl Buffer {
    pub fn new(
        device: &Device,
        label: &str,
        size: u64,
        usage: BufferUsages,
        location: MemoryLocation,
    ) -> Self {
        Self::try_new(device, label, size, usage, location).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_new(
        _device: &Device,
        label: &str,
        size: u64,
        usage: BufferUsages,
        location: MemoryLocation,
    ) -> Result<Self, String> {
        if size >= 1 << 53 {
            return Err(format!("buffer {label} is too large"));
        }
        let readback = matches!(location, MemoryLocation::Readback);
        let id = create_buffer(label, size as f64, usage.0, readback);
        if matches!(location, MemoryLocation::Host) {
            registry(|r| r.shadows.insert(id, vec![0; padded(size)]));
        }
        Ok(Self { id, size, readback })
    }

    pub fn try_dedicated(
        device: &Device,
        label: &str,
        size: u64,
        usage: BufferUsages,
        location: MemoryLocation,
    ) -> Result<Self, String> {
        Self::try_new(device, label, size, usage, location)
    }

    pub fn with_data(device: &Device, label: &str, data: &[u8], usage: BufferUsages) -> Self {
        Self::try_with_data(device, label, data, usage).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_with_data(
        device: &Device,
        label: &str,
        data: &[u8],
        usage: BufferUsages,
    ) -> Result<Self, String> {
        let mut buffer = Self::try_new(device, label, data.len() as u64, usage, MemoryLocation::Host)?;
        buffer.write(data);
        Ok(buffer)
    }

    pub fn handle(&self) -> BufferHandle {
        BufferHandle(self.id)
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// A readback buffer's latest contents, which trail the GPU by a frame
    /// or more: WebGPU maps buffers asynchronously. Zeroes until the first
    /// copy arrives.
    pub fn read<R>(&mut self, read: impl FnOnce(&[u8]) -> R) -> R {
        if self.readback {
            let latest = read_buffer(self.id);
            if !latest.is_undefined() && !latest.is_null() {
                let bytes = js_sys::Uint8Array::new(&latest).to_vec();
                return read(&bytes[..(self.size as usize).min(bytes.len())]);
            }
            return read(&vec![0; self.size as usize]);
        }
        registry(|r| match r.shadows.get(&self.id) {
            Some(shadow) => read(&shadow[..self.size as usize]),
            None => read(&vec![0; self.size as usize]),
        })
    }

    pub fn write(&mut self, data: &[u8]) {
        self.write_at(0, data);
    }

    /// WebGPU writes whole four-byte words, so the words `data` touches are
    /// written from the host copy.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) {
        let id = self.id;
        registry(|r| {
            let shadow = r.shadows.get_mut(&id).expect("buffer is not host-mapped");
            shadow[offset..offset + data.len()].copy_from_slice(data);
            let start = offset & !3;
            let end = (offset + data.len()).div_ceil(4) * 4;
            write_buffer(id, start as f64, &shadow[start..end.min(shadow.len())]);
        });
    }

    pub fn zero(&mut self) {
        let id = self.id;
        registry(|r| {
            if let Some(shadow) = r.shadows.get_mut(&id) {
                shadow.fill(0);
                write_buffer(id, 0.0, shadow);
            }
        });
    }

    pub fn destroy(self, _device: &Device) {
        if self.id != 0 {
            registry(|r| r.shadows.remove(&self.id));
            js_destroy(self.id);
        }
    }
}

// ---- Textures and samplers ----

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct TextureHandle {
    texture: u32,
    view: u32,
}

#[derive(Default)]
pub struct Texture {
    id: u32,
    view: u32,
    /// A view per level when there are several ([`Self::mip_handle`]).
    mip_views: Vec<u32>,
}

fn view_dimension(dimension: TextureViewDimension) -> &'static str {
    match dimension {
        TextureViewDimension::D2 => "2d",
        TextureViewDimension::D2Array => "2d-array",
        TextureViewDimension::Cube => "cube",
    }
}

impl Texture {
    pub fn new(device: &Device, desc: &TextureDesc<'_>) -> Self {
        Self::try_new(device, desc).unwrap_or_else(|e| panic!("{e}"))
    }

    pub fn try_new(_device: &Device, desc: &TextureDesc<'_>) -> Result<Self, String> {
        let format = texture_format(desc.format);
        let renderable = desc.usage.contains(TextureUsages::RENDER_ATTACHMENT)
            || !matches!(
                desc.format,
                TextureFormat::Rg11b10Float | TextureFormat::Rgba16Unorm
            );
        let id = create_texture(
            desc.label,
            desc.width,
            desc.height,
            desc.layers,
            desc.mip_levels,
            format,
            renderable,
        );
        let dimension = view_dimension(desc.dimension);
        let mut view_of = |first_mip, mips| {
            let view = create_view(id, dimension, first_mip, mips, 0, desc.layers);
            registry(|r| {
                r.views.insert(
                    view,
                    ViewInfo {
                        texture: id,
                        format,
                        dimension,
                    },
                )
            });
            view
        };
        let view = view_of(0, desc.mip_levels);
        let mip_views = if desc.mip_levels > 1 {
            (0..desc.mip_levels).map(|level| view_of(level, 1)).collect()
        } else {
            Vec::new()
        };
        registry(|r| r.texture_formats.insert(id, format));
        Ok(Self {
            id,
            view,
            mip_views,
        })
    }

    pub fn handle(&self) -> TextureHandle {
        TextureHandle {
            texture: self.id,
            view: self.view,
        }
    }

    pub fn mip_handle(&self, level: u32) -> TextureHandle {
        TextureHandle {
            texture: self.id,
            view: match self.mip_views.get(level as usize) {
                Some(&view) => view,
                None => {
                    assert!(
                        self.mip_views.is_empty() && level == 0,
                        "no mip level {level}"
                    );
                    self.view
                }
            },
        }
    }

    pub fn destroy(self, _device: &Device) {
        if self.id == 0 {
            return;
        }
        registry(|r| {
            for view in self.mip_views.iter().chain([&self.view]) {
                r.views.remove(view);
            }
            r.texture_formats.remove(&self.id);
        });
        for &view in self.mip_views.iter().chain([&self.view]) {
            js_destroy(view);
        }
        js_destroy(self.id);
    }
}

pub fn texture_format(format: TextureFormat) -> Format {
    match format {
        TextureFormat::R8Unorm => "r8unorm",
        TextureFormat::Rg8Unorm => "rg8unorm",
        TextureFormat::Rgba8Unorm => "rgba8unorm",
        TextureFormat::Rgba16Unorm => "rgba16unorm",
        TextureFormat::Rgb10a2Unorm => "rgb10a2unorm",
        TextureFormat::R16Float => "r16float",
        TextureFormat::Rg16Float => "rg16float",
        TextureFormat::Rgba16Float => "rgba16float",
        TextureFormat::R32Float => "r32float",
        TextureFormat::Rg32Float => "rg32float",
        TextureFormat::Rgba32Float => "rgba32float",
        TextureFormat::Rg11b10Float => "rg11b10ufloat",
        TextureFormat::Depth32Float => "depth32float",
    }
}

fn texel_size(format: Format) -> u32 {
    match format {
        "r8unorm" => 1,
        "rg8unorm" | "r16float" => 2,
        "rgba16unorm" | "rgba16float" | "rg32float" => 8,
        "rgba32float" => 16,
        _ => 4,
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Sampler(u32);

impl Sampler {
    pub fn destroy(self, _device: &Device) {
        if self.0 != 0 {
            registry(|r| r.samplers.remove(&self.0));
            js_destroy(self.0);
        }
    }
}

pub fn create_sampler(device: &Device, desc: &SamplerDesc) -> Sampler {
    try_create_sampler(device, desc).unwrap_or_else(|e| panic!("{e}"))
}

pub fn try_create_sampler(_device: &Device, desc: &SamplerDesc) -> Result<Sampler, String> {
    let filter = |mode| match mode {
        FilterMode::Nearest => "nearest",
        FilterMode::Linear => "linear",
    };
    let address = match desc.address_mode {
        AddressMode::ClampToEdge => "clamp-to-edge",
        AddressMode::Repeat => "repeat",
    };
    let compare = desc.compare.map_or("", compare_function);
    let id = create_sampler_js(
        filter(desc.filter),
        address,
        filter(desc.mipmap_filter),
        desc.lod_max_clamp,
        compare,
    );
    registry(|r| r.samplers.insert(id, desc.compare.is_some()));
    Ok(Sampler(id))
}

fn compare_function(compare: CompareFunction) -> &'static str {
    match compare {
        CompareFunction::Less => "less",
        CompareFunction::LessEqual => "less-equal",
        CompareFunction::Greater => "greater",
        CompareFunction::Always => "always",
    }
}

// ---- Bind groups and layouts ----

#[derive(Clone, Copy)]
pub enum BindingResource {
    UniformBuffer { buffer: BufferHandle, size: u64 },
    StorageBuffer { buffer: BufferHandle, size: u64 },
    Texture(TextureHandle),
    Sampler(Sampler),
}

impl BindingResource {
    pub fn uniform(buffer: &Buffer, size: u64) -> Self {
        Self::UniformBuffer {
            buffer: buffer.handle(),
            size,
        }
    }

    pub fn storage(buffer: &Buffer, size: u64) -> Self {
        Self::StorageBuffer {
            buffer: buffer.handle(),
            size,
        }
    }
}

/// What a bind group binds at an entry, which settles what its layout
/// must say where no shader has yet.
#[derive(Clone, Copy)]
enum Actual {
    Buffer,
    Texture { depth: bool, dimension: &'static str },
    Sampler { compare: bool },
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct BindGroupLayout(u32);

impl BindGroupLayout {
    pub fn destroy(self, _device: &Device) {}

    /// The WebGPU layout for this layout's entries, as the shaders that use
    /// it declare them and as `actual` binds them; equal descriptors share
    /// one layout object.
    fn resolve(self, actual: &[(u32, Actual)]) -> u32 {
        let json = registry(|r| {
            let info = &mut r.layouts[self.0 as usize - 1];
            for &(binding, a) in actual {
                if let Actual::Texture { depth: true, .. } = a
                    && let Some(seen) = info.depth_seen.get_mut(binding as usize)
                {
                    *seen = true;
                }
            }
            let any_compare = actual
                .iter()
                .any(|(_, a)| matches!(a, Actual::Sampler { compare: true }));
            let entries: Vec<Value> = info
                .entries
                .iter()
                .enumerate()
                .map(|(binding, entry)| {
                    let actual = actual
                        .iter()
                        .find(|(b, _)| *b as usize == binding)
                        .map(|&(_, a)| a);
                    layout_entry(
                        binding as u32,
                        entry,
                        info.reflected[binding].as_ref(),
                        actual,
                        info.depth_seen[binding],
                        any_compare,
                    )
                })
                .collect();
            Value::Array(entries).to_string()
        });
        if let Some(&id) = registry(|r| r.js_layouts.get(&json).copied()).as_ref() {
            return id;
        }
        let id = create_bind_group_layout_js(&json);
        registry(|r| r.js_layouts.insert(json, id));
        id
    }
}

fn stage_bits(stages: ShaderStages) -> u32 {
    let mut bits = 0;
    if stages.contains(ShaderStages::VERTEX) {
        bits |= 1;
    }
    if stages.contains(ShaderStages::FRAGMENT) {
        bits |= 2;
    }
    if stages.contains(ShaderStages::COMPUTE) {
        bits |= 4;
    }
    bits
}

fn layout_entry(
    binding: u32,
    entry: &BindGroupLayoutEntry,
    reflected: Option<&Reflected>,
    actual: Option<Actual>,
    depth_seen: bool,
    group_compares: bool,
) -> Value {
    let visibility = stage_bits(entry.visibility);
    let kind = reflected.map(|r| r.kind.as_str());
    let resource = match entry.ty {
        BindingType::UniformBuffer => json!({"buffer": {"type": "uniform"}}),
        BindingType::StorageBuffer => {
            let writable = match kind {
                Some("sw") => true,
                Some(_) => false,
                None => entry.visibility.contains(ShaderStages::COMPUTE),
            };
            json!({"buffer": {"type": if writable { "storage" } else { "read-only-storage" }}})
        }
        BindingType::Texture => {
            let (actual_depth, actual_dim) = match actual {
                Some(Actual::Texture { depth, dimension }) => (depth, Some(dimension)),
                _ => (depth_seen, None),
            };
            let sample_type = match kind {
                Some("td") => "depth",
                Some("tu") => "uint",
                Some("ti") => "sint",
                Some(_) if actual_depth => "unfilterable-float",
                Some(_) => "float",
                None if actual_depth && group_compares => "depth",
                None if actual_depth => "unfilterable-float",
                None => "float",
            };
            let dimension = reflected
                .and_then(|r| r.dimension.clone())
                .or(actual_dim.map(str::to_owned))
                .unwrap_or_else(|| "2d".into());
            json!({"texture": {"sampleType": sample_type, "viewDimension": dimension}})
        }
        BindingType::Sampler => {
            let compare = match (kind, actual) {
                (Some("sc"), _) => true,
                (Some(_), _) => false,
                (None, Some(Actual::Sampler { compare })) => compare,
                (None, _) => false,
            };
            json!({"sampler": {"type": if compare { "comparison" } else { "filtering" }}})
        }
    };
    let mut value = json!({"binding": binding, "visibility": visibility});
    value
        .as_object_mut()
        .unwrap()
        .extend(resource.as_object().unwrap().clone());
    value
}

pub fn create_bind_group_layout(
    _device: &Device,
    entries: &[BindGroupLayoutEntry],
) -> BindGroupLayout {
    registry(|r| {
        r.layouts.push(LayoutInfo {
            entries: entries.to_vec(),
            reflected: vec![None; entries.len()],
            depth_seen: vec![false; entries.len()],
        });
        BindGroupLayout(r.layouts.len() as u32)
    })
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineLayout(u32);

impl PipelineLayout {
    pub fn destroy(self, _device: &Device) {}
}

pub fn create_pipeline_layout(
    _device: &Device,
    bind_group_layouts: &[BindGroupLayout],
    push_constants: Option<PushConstants>,
) -> PipelineLayout {
    registry(|r| {
        r.pipeline_layouts.push(PipelineLayoutInfo {
            groups: bind_group_layouts.to_vec(),
            push: push_constants,
        });
        PipelineLayout(r.pipeline_layouts.len() as u32)
    })
}

/// A bind group, which WebGPU can't change after making it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct BindGroup(u32);

impl BindGroup {
    pub fn destroy(self, _device: &Device) {
        if self.0 != 0 {
            js_destroy(self.0);
        }
    }

    pub fn replace(
        &mut self,
        device: &Device,
        layout: BindGroupLayout,
        bindings: &[(u32, BindingResource)],
    ) {
        std::mem::replace(self, device.create_bind_group(layout, bindings)).destroy(device);
    }
}

// ---- Shaders and pipelines ----

/// A shader's bindings from its `// pomme-bindings` header, as
/// `(group, binding, reflected)`.
fn shader_bindings(wgsl: &str) -> Vec<(u32, u32, Reflected)> {
    let Some(header) = wgsl.lines().next().and_then(|l| l.strip_prefix("// pomme-bindings")) else {
        return Vec::new();
    };
    header
        .split_whitespace()
        .filter_map(|entry| {
            let mut parts = entry.split(':');
            let group = parts.next()?.parse().ok()?;
            let binding = parts.next()?.parse().ok()?;
            let kind = parts.next()?.to_owned();
            let dimension = parts.next().map(str::to_owned);
            Some((group, binding, Reflected { kind, dimension }))
        })
        .collect()
}

fn wgsl<'a>(label: &str, shader: &'a [u8]) -> Result<&'a str, String> {
    std::str::from_utf8(shader).map_err(|_| format!("{label}: not a WGSL shader"))
}

/// The shader module for `shader`, its push uniform moved to `push_group`.
fn shader_module(label: &str, shader: &str, push_group: i32) -> u32 {
    let key = (shader.as_ptr() as usize, shader.len(), push_group);
    if let Some(id) = registry(|r| r.modules.get(&key).copied()) {
        return id;
    }
    let code = if push_group >= 0 {
        shader.replace("@group(9) @binding(0)", &format!("@group({push_group}) @binding(0)"))
    } else {
        shader.to_owned()
    };
    let id = create_shader_module(label, &code);
    registry(|r| r.modules.insert(key, id));
    id
}

/// The WebGPU pipeline layout for `layout`, the shaders' bindings folded
/// into its bind group layouts first; and the group the push uniform takes.
fn resolve_pipeline_layout(layout: PipelineLayout, shaders: &[&str]) -> Result<(u32, i32), String> {
    let (groups, push) = registry(|r| {
        let info = &r.pipeline_layouts[layout.0 as usize - 1];
        (info.groups.clone(), info.push)
    });
    for shader in shaders {
        for (group, binding, reflected) in shader_bindings(shader) {
            let Some(layout) = groups.get(group as usize) else {
                continue;
            };
            registry(|r| {
                let info = &mut r.layouts[layout.0 as usize - 1];
                if let Some(slot) = info.reflected.get_mut(binding as usize) {
                    *slot = Some(reflected);
                }
            });
        }
    }
    let push_group = match push {
        Some(_) if groups.len() >= 4 => {
            return Err("a pipeline with four bind groups has no group left for push constants".into());
        }
        Some(_) => groups.len() as i32,
        None => -1,
    };
    let js_groups: Vec<u32> = groups.iter().map(|g| g.resolve(&[])).collect();
    let key = (js_groups.clone(), push_group);
    if let Some(id) = registry(|r| r.js_pipeline_layouts.get(&key).copied()) {
        return Ok((id, push_group));
    }
    let id = create_pipeline_layout_js(&js_groups, push_group);
    registry(|r| r.js_pipeline_layouts.insert(key, id));
    Ok((id, push_group))
}

macro_rules! pipeline_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Default)]
        pub struct $name(u32);

        impl $name {
            pub fn destroy(self, _device: &Device) {
                if self.0 != 0 {
                    js_destroy(self.0);
                }
            }
        }
    };
}

pipeline_type!(RenderPipeline);
pipeline_type!(ComputePipeline);

pub type PipelineTargets = super::PipelineTargets<Format>;

pub fn create_compute_pipeline(
    _device: &Device,
    layout: PipelineLayout,
    label: &str,
    shader: &[u8],
) -> ComputePipeline {
    let shader = wgsl(label, shader).unwrap_or_else(|e| panic!("{e}"));
    let (js_layout, push_group) =
        resolve_pipeline_layout(layout, &[shader]).unwrap_or_else(|e| panic!("{label}: {e}"));
    let module = shader_module(label, shader, push_group);
    ComputePipeline(create_compute_pipeline_js(label, js_layout, module))
}

pub fn create_render_pipeline(
    device: &Device,
    targets: PipelineTargets,
    layout: PipelineLayout,
    desc: &RenderPipelineDesc,
) -> RenderPipeline {
    try_create_render_pipeline(device, targets, layout, desc)
        .unwrap_or_else(|e| panic!("failed to create {} pipeline: {e}", desc.label))
}

pub fn try_create_render_pipeline(
    _device: &Device,
    targets: PipelineTargets,
    layout: PipelineLayout,
    desc: &RenderPipelineDesc,
) -> Result<RenderPipeline, String> {
    let vertex = wgsl(desc.label, desc.vertex_shader)?;
    let fragment = if desc.fragment_shader.is_empty() {
        None
    } else {
        Some(wgsl(desc.label, desc.fragment_shader)?)
    };
    let shaders: Vec<&str> = std::iter::once(vertex).chain(fragment).collect();
    let (js_layout, push_group) = resolve_pipeline_layout(layout, &shaders)?;
    let vertex_module = shader_module(desc.label, vertex, push_group);
    let fragment_module = fragment.map_or(0, |f| shader_module(desc.label, f, push_group));
    let json = pipeline_descriptor(desc, targets, fragment.is_some()).to_string();
    Ok(RenderPipeline(create_render_pipeline_js(
        &json,
        js_layout,
        vertex_module,
        fragment_module,
    )))
}

fn pipeline_descriptor(desc: &RenderPipelineDesc<'_>, targets: PipelineTargets, fragment: bool) -> Value {
    let buffers: Vec<Value> = desc
        .vertex_buffers
        .iter()
        .map(|buffer| {
            let attributes: Vec<Value> = buffer
                .attributes
                .iter()
                .map(|a| {
                    json!({"shaderLocation": a.location, "offset": a.offset,
                        "format": vertex_format(a.format)})
                })
                .collect();
            json!({
                "arrayStride": buffer.stride,
                "stepMode": match buffer.step_mode {
                    VertexStepMode::Vertex => "vertex",
                    VertexStepMode::Instance => "instance",
                },
                "attributes": attributes,
            })
        })
        .collect();
    let mut result = json!({
        "label": desc.label,
        "vertex": {"entryPoint": "main", "buffers": buffers},
        "primitive": {
            "topology": match desc.topology {
                Topology::TriangleList => "triangle-list",
                Topology::LineList => "line-list",
            },
            "cullMode": match desc.cull_mode {
                CullMode::None => "none",
                CullMode::Front => "front",
                CullMode::Back => "back",
            },
            "frontFace": match desc.front_face {
                FrontFace::Ccw => "ccw",
                FrontFace::Cw => "cw",
            },
            "unclippedDepth": desc.depth_clamp,
        },
    });
    if fragment {
        let mut colors: Vec<Value> = std::iter::once(targets.color)
            .chain(targets.extra_colors)
            .map(|format| {
                format.map_or(Value::Null, |format| {
                    let mut target = json!({"format": format,
                        "writeMask": if desc.write_color { 15 } else { 0 }});
                    if let Some(blend) = desc.blend {
                        target["blend"] = json!({"color": blend_component(blend.color),
                            "alpha": blend_component(blend.alpha)});
                    }
                    target
                })
            })
            .collect();
        while colors.last().is_some_and(Value::is_null) {
            colors.pop();
        }
        result["fragment"] = json!({"entryPoint": "main", "targets": colors});
    }
    if targets.depth {
        let mut depth = json!({
            "format": "depth32float",
            "depthWriteEnabled": desc.depth.is_some_and(|d| d.write),
            "depthCompare": desc.depth.map_or("always", |d| compare_function(d.compare)),
        });
        if let Some(bias) = desc.depth.and_then(|d| d.bias) {
            depth["depthBias"] = json!(bias.constant);
            depth["depthBiasSlopeScale"] = json!(bias.slope);
        }
        result["depthStencil"] = depth;
    }
    result
}

fn blend_component(value: BlendComponent) -> Value {
    let factor = |f| match f {
        BlendFactor::Zero => "zero",
        BlendFactor::One => "one",
        BlendFactor::SrcColor => "src",
        BlendFactor::OneMinusSrcColor => "one-minus-src",
        BlendFactor::DstColor => "dst",
        BlendFactor::OneMinusDstColor => "one-minus-dst",
        BlendFactor::SrcAlpha => "src-alpha",
        BlendFactor::OneMinusSrcAlpha => "one-minus-src-alpha",
    };
    json!({"srcFactor": factor(value.src), "dstFactor": factor(value.dst), "operation": "add"})
}

fn vertex_format(format: VertexFormat) -> &'static str {
    match format {
        VertexFormat::Float32 => "float32",
        VertexFormat::Float32x2 => "float32x2",
        VertexFormat::Float32x3 => "float32x3",
        VertexFormat::Float32x4 => "float32x4",
        VertexFormat::Sint32x3 => "sint32x3",
        VertexFormat::Uint32 => "uint32",
        VertexFormat::Sint16x2 => "sint16x2",
        VertexFormat::Uint16 => "uint16",
        VertexFormat::Unorm16 => "unorm16",
        VertexFormat::Unorm16x2 => "unorm16x2",
        VertexFormat::Uint8x2 => "uint8x2",
        VertexFormat::Unorm8x4 => "unorm8x4",
        VertexFormat::Snorm8x4 => "snorm8x4",
    }
}

// ---- Commands ----

pub type TextureTransition = super::TextureTransition<TextureHandle>;
pub type ColorAttachment = super::ColorAttachment<TextureHandle, Format>;
pub type DepthAttachment = super::DepthAttachment<TextureHandle>;
pub type RenderPassDesc = super::RenderPassDesc<TextureHandle, Format>;

/// Records commands, in the shape of WebGPU's `GPUCommandEncoder`. WebGPU
/// tracks resource use itself, so transitions do nothing.
#[derive(Clone, Copy)]
pub struct CommandEncoder {
    id: u32,
}

impl CommandEncoder {
    fn begin(label: &str) -> Self {
        Self {
            id: begin_encoder(label),
        }
    }

    pub fn transition_textures(&self, _transitions: &[TextureTransition]) {}

    pub fn transition_buffers(&self, _from: BufferUse, _to: BufferUse) {}

    pub fn transition_buffers_many(&self, _transitions: &[(BufferUse, BufferUse)]) {}

    pub fn fill_buffer(&self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        clear_buffer_js(self.id, buffer.0, offset as f64, size as f64, value);
    }

    pub fn copy_buffer(&self, src: BufferHandle, dst: BufferHandle, copies: &[BufferCopy]) {
        for copy in copies {
            copy_buffer_js(
                self.id,
                src.0,
                copy.src_offset as f64,
                dst.0,
                copy.dst_offset as f64,
                copy.size as f64,
            );
        }
    }

    /// WebGPU copies rows of a multiple of 256 bytes from buffers; others
    /// are written from the host copy of a host buffer, ahead of the
    /// encoder's commands.
    pub fn copy_buffer_to_texture(
        &self,
        buffer: BufferHandle,
        image: TextureHandle,
        copies: &[TexelCopy],
    ) {
        let texel = texel_size(registry(|r| {
            r.texture_formats
                .get(&image.texture)
                .copied()
                .unwrap_or("rgba8unorm")
        }));
        for copy in copies {
            let row = copy.width * texel;
            if row % 256 == 0 {
                copy_buffer_to_texture_js(
                    self.id,
                    buffer.0,
                    copy.buffer_offset as f64,
                    row,
                    image.texture,
                    copy.mip_level,
                    copy.layer,
                    copy.origin[0],
                    copy.origin[1],
                    copy.width,
                    copy.height,
                );
                continue;
            }
            let written = registry(|r| {
                let Some(shadow) = r.shadows.get(&buffer.0) else {
                    return false;
                };
                let start = copy.buffer_offset as usize;
                let end = start + (row * copy.height) as usize;
                write_texture(
                    image.texture,
                    copy.mip_level,
                    copy.layer,
                    copy.origin[0],
                    copy.origin[1],
                    copy.width,
                    copy.height,
                    row,
                    &shadow[start..end],
                );
                true
            });
            if !written {
                warn_once("a texture upload from a GPU buffer with unaligned rows was skipped");
            }
        }
    }

    pub fn copy_texture_to_buffer(
        &self,
        image: TextureHandle,
        buffer: BufferHandle,
        copies: &[TexelCopy],
    ) {
        let texel = texel_size(registry(|r| {
            r.texture_formats
                .get(&image.texture)
                .copied()
                .unwrap_or("rgba8unorm")
        }));
        for copy in copies {
            let row = copy.width * texel;
            if row % 256 != 0 {
                warn_once("a texture readback with unaligned rows was skipped");
                continue;
            }
            copy_texture_to_buffer_js(
                self.id,
                image.texture,
                copy.mip_level,
                copy.layer,
                copy.origin[0],
                copy.origin[1],
                copy.width,
                copy.height,
                buffer.0,
                copy.buffer_offset as f64,
                row,
            );
        }
    }

    pub fn blit(&self, src: TextureHandle, dst: TextureHandle, blit: &Blit) {
        blit_js(
            self.id,
            src.texture,
            dst.texture,
            blit.src_level,
            blit.dst_level,
            blit.layers,
        );
    }

    pub fn clear_texture(&self, image: TextureHandle, range: TextureRange, color: [f32; 4]) {
        clear_texture_js(
            self.id,
            image.texture,
            range.first_mip,
            range.mips,
            range.first_layer,
            range.layers,
            color[0],
            color[1],
            color[2],
            color[3],
        );
    }

    pub fn clear_depth(&self, image: TextureHandle, depth: f32) {
        clear_texture_js(self.id, image.texture, 0, 1, 0, 1, depth, 0.0, 0.0, 0.0);
    }

    pub fn copy_depth(&self, src: TextureHandle, dst: TextureHandle, extent: Extent) {
        copy_texture_js(self.id, src.texture, dst.texture, extent.width, extent.height);
    }

    pub fn upload_texture(
        &self,
        staging: BufferHandle,
        image: TextureHandle,
        _range: TextureRange,
        copies: &[TexelCopy],
    ) {
        self.copy_buffer_to_texture(staging, image, copies);
    }

    pub fn begin_render_pass(&self, _device: &Device, desc: &RenderPassDesc) -> RenderPassEncoder {
        let colors = Array::new();
        let mut push_color = |attachment: Option<&ColorAttachment>| {
            let Some(a) = attachment else {
                for _ in 0..8 {
                    colors.push(&JsValue::from(0));
                }
                return;
            };
            let (load, clear) = match a.load {
                LoadOp::Load => (1, [0.0; 4]),
                LoadOp::Clear(c) => (0, c),
            };
            colors.push(&JsValue::from(a.texture.view));
            colors.push(&JsValue::from_str(a.format));
            colors.push(&JsValue::from(load));
            colors.push(&JsValue::from(matches!(a.store, StoreOp::Store) as u32));
            for c in clear {
                colors.push(&JsValue::from(c));
            }
        };
        if desc.color.is_some() || !desc.extra_colors.is_empty() {
            push_color(desc.color.as_ref());
        }
        for extra in &desc.extra_colors {
            push_color(Some(extra));
        }
        let depth: Vec<f64> = desc.depth.map_or_else(Vec::new, |d| {
            let (load, value) = match d.load {
                LoadOp::Load => (1.0, 1.0),
                LoadOp::Clear(v) => (0.0, v as f64),
            };
            vec![
                d.texture.view as f64,
                load,
                matches!(d.store, StoreOp::Store) as u32 as f64,
                value,
            ]
        });
        RenderPassEncoder {
            id: begin_render_pass_js(self.id, &colors, &depth, desc.extent.width, desc.extent.height),
        }
    }

    pub fn dispatch(
        &self,
        pipeline: &ComputePipeline,
        group: BindGroup,
        push: &[u8],
        groups: [u32; 3],
    ) {
        dispatch_js(
            self.id, pipeline.0, group.0, push, groups[0], groups[1], groups[2], 0, 0.0,
        );
    }

    pub fn dispatch_indirect(
        &self,
        pipeline: &ComputePipeline,
        group: BindGroup,
        push: &[u8],
        buffer: BufferHandle,
        offset: u64,
    ) {
        dispatch_js(
            self.id,
            pipeline.0,
            group.0,
            push,
            0,
            0,
            0,
            buffer.0,
            offset as f64,
        );
    }
}

/// Records one render pass, in the shape of WebGPU's
/// `GPURenderPassEncoder`.
pub struct RenderPassEncoder {
    id: u32,
}

impl RenderPassEncoder {
    pub fn end(self) {
        end_pass(self.id);
    }

    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        set_pipeline_js(self.id, pipeline.0);
    }

    pub fn set_bind_groups(&self, first: u32, groups: &[BindGroup]) {
        for (i, group) in groups.iter().enumerate() {
            set_bind_group_js(self.id, first + i as u32, group.0);
        }
    }

    pub fn set_bind_group(&self, index: u32, group: BindGroup) {
        set_bind_group_js(self.id, index, group.0);
    }

    pub fn set_push_constants(&self, _visibility: ShaderStages, offset: u32, data: &[u8]) {
        set_push(self.id, offset, data);
    }

    pub fn set_vertex_buffer(&self, slot: u32, buffer: BufferHandle) {
        set_vertex_buffer_js(self.id, slot, buffer.0);
    }

    pub fn set_index_buffer(&self, buffer: BufferHandle) {
        set_index_buffer_js(self.id, buffer.0);
    }

    pub fn set_viewport(&self, rect: Rect) {
        set_viewport_js(
            self.id,
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
        );
    }

    pub fn set_scissor_rect(&self, rect: Rect) {
        set_scissor_js(self.id, rect.x, rect.y, rect.width, rect.height);
    }

    pub fn draw(&self, vertex_count: u32, instance_count: u32, first_vertex: u32, first_instance: u32) {
        draw_js(self.id, vertex_count, instance_count, first_vertex, first_instance);
    }

    pub fn draw_indexed(
        &self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    ) {
        draw_indexed_js(
            self.id,
            index_count,
            instance_count,
            first_index,
            base_vertex,
            first_instance,
        );
    }

    pub fn draw_indexed_indirect(&self, buffer: BufferHandle, offset: u64, draw_count: u32, stride: u32) {
        draw_indexed_indirect_js(self.id, buffer.0, offset as f64, draw_count, stride);
    }

    /// WebGPU reads no draw count from a buffer: all `max_draws` are drawn,
    /// the unfilled ones zeroed into no-op draws.
    pub fn draw_indexed_indirect_count(
        &self,
        buffer: BufferHandle,
        offset: u64,
        _count_buffer: BufferHandle,
        _count_offset: u64,
        max_draws: u32,
        stride: u32,
    ) {
        self.draw_indexed_indirect(buffer, offset, max_draws, stride);
    }

    pub fn clear_depth(&self, rect: Rect) {
        clear_rect(self.id, true, rect.x, rect.y, rect.width, rect.height, 0.0, 0.0, 0.0, 0.0);
    }

    pub fn clear_color(&self, rect: Rect, color: [f32; 4]) {
        clear_rect(
            self.id, false, rect.x, rect.y, rect.width, rect.height, color[0], color[1], color[2],
            color[3],
        );
    }
}

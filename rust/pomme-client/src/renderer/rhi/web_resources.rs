//! Resource creation from the shared RHI descriptions, without Vulkan types.
use wasm_bindgen::JsValue;

use super::web_context::BrowserGpuContext;
use super::{
    AddressMode, BufferUsages, CompareFunction, FilterMode, MemoryLocation, SamplerDesc, TextureDesc,
    TextureFormat, TextureRange, TextureViewDimension,
};

fn size_number(value: u64) -> Result<f64, JsValue> {
    if value > (1 << 53) - 1 {
        return Err(JsValue::from_str("WebGPU size exceeds JavaScript integer precision"));
    }
    Ok(value as f64)
}

pub struct BrowserBuffer {
    raw: JsValue,
    size: u64,
}

impl BrowserBuffer {
    pub fn new(
        context: &BrowserGpuContext,
        label: &str,
        size: u64,
        usage: BufferUsages,
        memory: MemoryLocation,
    ) -> Result<Self, JsValue> {
        let memory = match memory {
            MemoryLocation::Host => 0,
            MemoryLocation::Device => 1,
            MemoryLocation::Readback => 2,
        };
        let raw = context.create_buffer_raw(label, size_number(size)?, usage.0, memory)?;
        Ok(Self { raw, size })
    }

    pub fn raw(&self) -> &JsValue {
        &self.raw
    }

    pub fn write(&self, device: &BrowserGpuContext, offset: u64, bytes: &[u8]) -> Result<(), JsValue> {
        let end = offset.checked_add(bytes.len() as u64);
        // Padding may touch bytes past the logical end only, never adjacent
        // live buffer data. WebGPU requires four-byte update granularity.
        if end.is_none_or(|end| end > self.size || (bytes.len() % 4 != 0 && end != self.size)) {
            return Err(JsValue::from_str("WebGPU buffer write out of range or unaligned"));
        }
        device.write_buffer_raw(&self.raw, size_number(offset)?, &js_sys::Uint8Array::from(bytes))
    }
}

pub fn create_texture(device: &BrowserGpuContext, desc: &TextureDesc<'_>) -> Result<JsValue, JsValue> {
    device.create_texture_raw(
        desc.label, desc.width, desc.height, desc.layers, desc.mip_levels,
        texture_format(desc.format), desc.usage.0,
    )
}

pub fn create_view(
    device: &BrowserGpuContext,
    texture: &JsValue,
    dimension: TextureViewDimension,
    range: TextureRange,
) -> Result<JsValue, JsValue> {
    let dimension = match dimension {
        TextureViewDimension::D2 => "2d",
        TextureViewDimension::D2Array => "2d-array",
        TextureViewDimension::Cube => "cube",
    };
    device.create_view_raw(texture, dimension, range.first_mip, range.mips, range.first_layer, range.layers)
}

pub fn create_sampler(device: &BrowserGpuContext, desc: SamplerDesc) -> Result<JsValue, JsValue> {
    fn filter(mode: FilterMode) -> &'static str {
        match mode {
            FilterMode::Nearest => "nearest",
            FilterMode::Linear => "linear",
        }
    }
    let address = match desc.address_mode {
        AddressMode::ClampToEdge => "clamp-to-edge",
        AddressMode::Repeat => "repeat",
    };
    let compare = match desc.compare {
        Some(CompareFunction::Less) => "less",
        Some(CompareFunction::LessEqual) => "less-equal",
        Some(CompareFunction::Greater) => "greater",
        Some(CompareFunction::Always) => "always",
        None => "",
    };
    device.create_sampler_raw(filter(desc.filter), address, filter(desc.mipmap_filter), desc.lod_max_clamp, compare)
}

pub(super) fn texture_format(format: TextureFormat) -> &'static str {
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

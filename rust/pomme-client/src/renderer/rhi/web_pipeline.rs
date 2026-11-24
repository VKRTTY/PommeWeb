//! Maps the shared renderer's pipeline descriptions to browser descriptors.
use serde_json::{Value, json};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::web_context::BrowserGpuContext;
use super::web_resources::texture_format;
use super::{
    BlendComponent, BlendFactor, CompareFunction, CullMode, FrontFace, PipelineTargets,
    RenderPipelineDesc, Topology, VertexFormat, VertexStepMode,
};

/// Attachment formats are names rather than TextureFormat because the surface
/// may choose BGRA, which isn't an internal RHI texture format.
pub fn descriptor(desc: &RenderPipelineDesc<'_>, targets: PipelineTargets<&str>) -> Value {
    let buffers = desc.vertex_buffers.iter().map(|buffer| {
        let attributes = buffer.attributes.iter().map(|attribute| json!({
            "shaderLocation": attribute.location,
            "offset": attribute.offset,
            "format": vertex_format(attribute.format),
        })).collect::<Vec<_>>();
        json!({
            "arrayStride": buffer.stride,
            "stepMode": match buffer.step_mode {
                VertexStepMode::Vertex => "vertex",
                VertexStepMode::Instance => "instance",
            },
            "attributes": attributes,
        })
    }).collect::<Vec<_>>();
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

    let mut colors = std::iter::once(targets.color)
        .chain(targets.extra_colors)
        .map(|format| format.map_or(Value::Null, |format| {
            let mut target = json!({"format": format, "writeMask": if desc.write_color { 15 } else { 0 }});
            if let Some(blend) = desc.blend {
                target["blend"] = json!({"color": blend_component(blend.color), "alpha": blend_component(blend.alpha)});
            }
            target
        }))
        .collect::<Vec<_>>();
    while colors.last().is_some_and(Value::is_null) {
        colors.pop();
    }
    if !desc.fragment_shader.is_empty() {
        result["fragment"] = json!({"entryPoint":"main", "targets":colors});
    }
    if targets.depth {
        let mut depth = json!({
            "format": texture_format(super::TextureFormat::Depth32Float),
            "depthWriteEnabled": desc.depth.is_some_and(|depth| depth.write),
            "depthCompare": desc.depth.map_or("always", |depth| compare(depth.compare)),
        });
        if let Some(bias) = desc.depth.and_then(|depth| depth.bias) {
            depth["depthBias"] = json!(bias.constant);
            depth["depthBiasSlopeScale"] = json!(bias.slope);
        }
        result["depthStencil"] = depth;
    }
    result
}

pub async fn create_render_pipeline(
    device: &BrowserGpuContext,
    desc: &RenderPipelineDesc<'_>,
    targets: PipelineTargets<&str>,
    vertex: &JsValue,
    fragment: Option<&JsValue>,
    layout: Option<&JsValue>,
) -> Result<JsValue, JsValue> {
    if desc.fragment_shader.is_empty() != fragment.is_none() {
        return Err(JsValue::from_str("Pomme WebGPU pipeline missing or unexpectedly given a fragment module"));
    }
    let json = descriptor(desc, targets).to_string();
    JsFuture::from(device.create_render_pipeline_raw(
        &json, vertex, fragment.unwrap_or(&JsValue::NULL), layout.unwrap_or(&JsValue::NULL),
    )?).await
}

fn compare(value: CompareFunction) -> &'static str {
    match value {
        CompareFunction::Less => "less",
        CompareFunction::LessEqual => "less-equal",
        CompareFunction::Greater => "greater",
        CompareFunction::Always => "always",
    }
}

fn blend_component(value: BlendComponent) -> Value {
    fn factor(value: BlendFactor) -> &'static str {
        match value {
            BlendFactor::Zero => "zero",
            BlendFactor::One => "one",
            BlendFactor::SrcColor => "src",
            BlendFactor::OneMinusSrcColor => "one-minus-src",
            BlendFactor::DstColor => "dst",
            BlendFactor::OneMinusDstColor => "one-minus-dst",
            BlendFactor::SrcAlpha => "src-alpha",
            BlendFactor::OneMinusSrcAlpha => "one-minus-src-alpha",
        }
    }
    json!({"srcFactor":factor(value.src),"dstFactor":factor(value.dst),"operation":"add"})
}

fn vertex_format(value: VertexFormat) -> &'static str {
    match value {
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

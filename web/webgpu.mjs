// WebGPU side of Pomme's browser render backend (rhi/web). Rust holds GPU
// objects as integer ids into `objs`; every call here is synchronous.
// Ids 1 and 2 are the current canvas texture's view and texture.

let device, context, canvas, format, features;
const objs = [null, null, null];
const freeIds = [];
const readbacks = new Map();
let errorCount = 0;

function put(o) {
  const id = freeIds.length ? freeIds.pop() : objs.length;
  objs[id] = o;
  return id;
}

const OPTIONAL_FEATURES = ['float32-filterable', 'rg11b10ufloat-renderable', 'depth-clip-control',
  'indirect-first-instance', 'texture-formats-tier1', 'unorm16-texture-formats', 'float32-blendable'];

export async function init(target) {
  canvas = target;
  if (!navigator.gpu) throw new Error('WebGPU is unavailable in this browser');
  const adapter = await navigator.gpu.requestAdapter({powerPreference: 'high-performance'});
  if (!adapter) throw new Error('No WebGPU adapter is available');
  const requiredFeatures = OPTIONAL_FEATURES.filter(f => adapter.features.has(f));
  const requiredLimits = {};
  for (const key in adapter.limits) {
    if (typeof adapter.limits[key] === 'number') requiredLimits[key] = adapter.limits[key];
  }
  try {
    device = await adapter.requestDevice({requiredFeatures, requiredLimits});
  } catch (e) {
    console.warn('Pomme: device with adapter limits refused, using defaults', e);
    device = await adapter.requestDevice({requiredFeatures});
  }
  features = device.features;
  device.addEventListener('uncapturederror', e => {
    if (++errorCount <= 50) console.error('WebGPU:', e.error.message);
  });
  device.lost.then(info => console.error('WebGPU device lost:', info.message));
  context = canvas.getContext('webgpu');
  if (!context) throw new Error('Could not create a WebGPU canvas context');
  format = navigator.gpu.getPreferredCanvasFormat();
  const info = adapter.info || {};
  return [info.vendor, info.architecture, info.device, info.description].filter(Boolean).join(' ') || 'WebGPU';
}

export function limit(name) { return device.limits[name] ?? 0; }
export function hasFeature(name) { return features.has(name); }

export function configure(width, height) {
  const max = device.limits.maxTextureDimension2D;
  canvas.width = Math.min(max, Math.max(1, width));
  canvas.height = Math.min(max, Math.max(1, height));
  context.configure({device, format, alphaMode: 'opaque',
    // Sampled too: the menu blur reads back what the frame drew so far.
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC
      | GPUTextureUsage.COPY_DST | GPUTextureUsage.TEXTURE_BINDING});
  return format;
}

export function canvasSize() { return [canvas.clientWidth, canvas.clientHeight]; }

export function acquire() {
  const texture = context.getCurrentTexture();
  objs[2] = texture;
  objs[1] = texture.createView();
}

export function destroy(id) {
  if (id < 3) return;
  const o = objs[id];
  if (o && (o instanceof GPUBuffer || o instanceof GPUTexture)) o.destroy();
  const r = readbacks.get(id);
  if (r) { readbacks.delete(id); r.dead = true; }
  objs[id] = null;
  freeIds.push(id);
}

// ---- Buffers ----

const BUFFER_USAGE = ['VERTEX', 'INDEX', 'UNIFORM', 'STORAGE', 'INDIRECT', 'COPY_SRC', 'COPY_DST'];

export function createBuffer(label, size, bits, readback) {
  let usage = GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST;
  BUFFER_USAGE.forEach((name, bit) => { if (bits & (1 << bit)) usage |= GPUBufferUsage[name]; });
  const buffer = device.createBuffer({label, size: Math.max(4, Math.ceil(size / 4) * 4), usage});
  const id = put(buffer);
  if (readback) readbacks.set(id, {staging: null, pending: false, latest: null, dead: false});
  return id;
}

export function writeBuffer(id, offset, bytes) {
  device.queue.writeBuffer(objs[id], offset, bytes);
}

/// The newest copy of a readback buffer's contents, or undefined before one
/// has arrived. Results trail the GPU by a frame or more.
export function readBuffer(id) { return readbacks.get(id)?.latest; }

function refreshReadbacks() {
  let encoder = null;
  const started = [];
  for (const [id, r] of readbacks) {
    if (r.pending) continue;
    const buffer = objs[id];
    if (!r.staging || r.staging.size !== buffer.size) {
      r.staging?.destroy();
      r.staging = device.createBuffer({size: buffer.size,
        usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST});
    }
    encoder ??= device.createCommandEncoder({label: 'readbacks'});
    encoder.copyBufferToBuffer(buffer, 0, r.staging, 0, buffer.size);
    started.push(r);
  }
  if (!encoder) return;
  device.queue.submit([encoder.finish()]);
  for (const r of started) {
    r.pending = true;
    const staging = r.staging;
    staging.mapAsync(GPUMapMode.READ).then(() => {
      if (!r.dead) r.latest = new Uint8Array(staging.getMappedRange().slice(0));
      staging.unmap();
      if (r.dead) staging.destroy();
      r.pending = false;
    }, () => { r.pending = false; });
  }
}

// ---- Textures and samplers ----

export function createTexture(label, width, height, layers, mips, fmt, renderable) {
  let usage = GPUTextureUsage.COPY_SRC | GPUTextureUsage.COPY_DST | GPUTextureUsage.TEXTURE_BINDING;
  if (renderable) usage |= GPUTextureUsage.RENDER_ATTACHMENT;
  return put(device.createTexture({label, size: [width, height, layers], mipLevelCount: mips,
    format: fmt, usage}));
}

export function createView(texture, dimension, baseMipLevel, mipLevelCount, baseArrayLayer, arrayLayerCount) {
  return put(objs[texture].createView({dimension, baseMipLevel, mipLevelCount, baseArrayLayer, arrayLayerCount}));
}

export function createSampler(filter, address, mipmap, maxLod, compare) {
  return put(device.createSampler({magFilter: filter, minFilter: filter, mipmapFilter: mipmap,
    addressModeU: address, addressModeV: address, addressModeW: address, lodMaxClamp: maxLod,
    ...(compare ? {compare} : {})}));
}

export function writeTexture(texture, mip, layer, x, y, width, height, bytesPerRow, bytes) {
  device.queue.writeTexture({texture: objs[texture], mipLevel: mip, origin: [x, y, layer]},
    bytes, {bytesPerRow, rowsPerImage: height}, [width, height, 1]);
}

// ---- Layouts, shaders and pipelines ----

export function createBindGroupLayout(json) {
  return put(device.createBindGroupLayout({entries: JSON.parse(json)}));
}

let pushLayout = null;
function getPushLayout() {
  pushLayout ??= device.createBindGroupLayout({label: 'push constants', entries: [{binding: 0,
    visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT | GPUShaderStage.COMPUTE,
    buffer: {type: 'uniform', hasDynamicOffset: true}}]});
  return pushLayout;
}

/// `groups` are bind group layout ids; `pushGroup` is -1 or the index the
/// former push constants bind at, with empty groups filling any gap.
export function createPipelineLayout(groups, pushGroup) {
  const layouts = Array.from(groups, id => objs[id]);
  if (pushGroup >= 0) {
    while (layouts.length < pushGroup) layouts.push(device.createBindGroupLayout({entries: []}));
    layouts.push(getPushLayout());
  }
  const layout = device.createPipelineLayout({bindGroupLayouts: layouts});
  return put({layout, userGroups: groups.length, pushGroup});
}

export function createShaderModule(label, code) {
  return put(device.createShaderModule({label, code}));
}

export function createRenderPipeline(json, layoutId, vertex, fragment) {
  const desc = JSON.parse(json);
  const info = objs[layoutId];
  desc.layout = info.layout;
  desc.vertex.module = objs[vertex];
  if (desc.fragment) desc.fragment.module = objs[fragment];
  if (desc.primitive.unclippedDepth && !features.has('depth-clip-control')) delete desc.primitive.unclippedDepth;
  if (desc.fragment && !features.has('float32-blendable')) {
    for (const t of desc.fragment.targets) {
      if (t && /32float$/.test(t.format)) delete t.blend;
    }
  }
  return put({pipeline: device.createRenderPipeline(desc), userGroups: info.userGroups, pushGroup: info.pushGroup});
}

export function createComputePipeline(label, layoutId, module) {
  const info = objs[layoutId];
  return put({pipeline: device.createComputePipeline({label, layout: info.layout,
    compute: {module: objs[module], entryPoint: 'main'}}), userGroups: info.userGroups, pushGroup: info.pushGroup});
}

/// `entries` holds [binding, kind, id, size] per entry; kind 0 is a buffer,
/// 1 a texture view, 2 a sampler.
export function createBindGroup(layout, entries) {
  const list = [];
  for (let i = 0; i < entries.length; i += 4) {
    const [binding, kind, id, size] = [entries[i], entries[i + 1], entries[i + 2], entries[i + 3]];
    const resource = kind === 0 ? {buffer: objs[id], offset: 0, size: Math.ceil(size / 4) * 4} : objs[id];
    list.push({binding, resource});
  }
  return put(device.createBindGroup({layout: objs[layout], entries: list}));
}

// ---- Former push constants ----
// Each draw's push block is a 256-byte slot of a uniform chunk, bound with a
// dynamic offset; an encoder's chunks are uploaded just before it submits.

const CHUNK = 64 * 1024;
const SLOT = 256;
const chunkPool = [];

function takeChunk() {
  const chunk = chunkPool.pop();
  if (chunk) return chunk;
  const buffer = device.createBuffer({label: 'push constants', size: CHUNK,
    usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST});
  const group = device.createBindGroup({layout: getPushLayout(),
    entries: [{binding: 0, resource: {buffer, offset: 0, size: SLOT}}]});
  return {buffer, group, cpu: new Uint8Array(CHUNK), used: 0};
}

class Encoder {
  constructor(label) {
    this.encoder = device.createCommandEncoder({label});
    this.chunks = [];
  }

  /// A slot holding `block`: [bind group, offset].
  push(block) {
    let chunk = this.chunks[this.chunks.length - 1];
    if (!chunk || chunk.used + SLOT > CHUNK) {
      chunk = takeChunk();
      this.chunks.push(chunk);
    }
    const offset = chunk.used;
    chunk.cpu.set(block, offset);
    chunk.used += SLOT;
    return [chunk.group, offset];
  }
}

export function beginEncoder(label) { return put(new Encoder(label)); }

export function submit(id, frame) {
  const e = objs[id];
  for (const chunk of e.chunks) {
    device.queue.writeBuffer(chunk.buffer, 0, chunk.cpu, 0, chunk.used);
  }
  device.queue.submit([e.encoder.finish()]);
  for (const chunk of e.chunks) { chunk.used = 0; chunkPool.push(chunk); }
  objs[id] = null;
  freeIds.push(id);
  if (frame) refreshReadbacks();
}

export function copyBuffer(enc, src, srcOffset, dst, dstOffset, size) {
  objs[enc].encoder.copyBufferToBuffer(objs[src], srcOffset, objs[dst], dstOffset, Math.ceil(size / 4) * 4);
}

export function clearBuffer(enc, buffer, offset, size, value) {
  const e = objs[enc].encoder;
  const bytes = Math.ceil(size / 4) * 4;
  if (value === 0) { e.clearBuffer(objs[buffer], offset, bytes); return; }
  const fill = device.createBuffer({size: bytes, usage: GPUBufferUsage.COPY_SRC, mappedAtCreation: true});
  new Uint32Array(fill.getMappedRange()).fill(value);
  fill.unmap();
  e.copyBufferToBuffer(fill, 0, objs[buffer], offset, bytes);
}

export function copyBufferToTexture(enc, buffer, offset, bytesPerRow, texture, mip, layer, x, y, width, height) {
  objs[enc].encoder.copyBufferToTexture({buffer: objs[buffer], offset, bytesPerRow, rowsPerImage: height},
    {texture: objs[texture], mipLevel: mip, origin: [x, y, layer]}, [width, height, 1]);
}

export function copyTextureToBuffer(enc, texture, mip, layer, x, y, width, height, buffer, offset, bytesPerRow) {
  objs[enc].encoder.copyTextureToBuffer({texture: objs[texture], mipLevel: mip, origin: [x, y, layer]},
    {buffer: objs[buffer], offset, bytesPerRow, rowsPerImage: height}, [width, height, 1]);
}

export function copyTexture(enc, src, dst, width, height) {
  objs[enc].encoder.copyTextureToTexture({texture: objs[src]}, {texture: objs[dst]}, [width, height, 1]);
}

function depthOnly(f) { return f === 'depth32float' || f === 'depth24plus' || f === 'depth16unorm'; }

export function clearTexture(enc, texture, firstMip, mips, firstLayer, layers, r, g, b, a) {
  const e = objs[enc].encoder;
  const t = objs[texture];
  for (let mip = firstMip; mip < firstMip + mips; mip++) {
    for (let layer = firstLayer; layer < firstLayer + layers; layer++) {
      const view = t.createView({dimension: '2d', baseMipLevel: mip, mipLevelCount: 1,
        baseArrayLayer: layer, arrayLayerCount: 1});
      const pass = depthOnly(t.format)
        ? e.beginRenderPass({colorAttachments: [], depthStencilAttachment: {view,
            depthLoadOp: 'clear', depthClearValue: r, depthStoreOp: 'store'}})
        : e.beginRenderPass({colorAttachments: [{view, loadOp: 'clear',
            clearValue: [r, g, b, a], storeOp: 'store'}]});
      pass.end();
    }
  }
}

const blitPipelines = new Map();
let blitSampler = null;
const BLIT_WGSL = `
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct V { @builtin(position) pos: vec4f, @location(0) uv: vec2f };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
  return V(vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0), uv);
}
@fragment fn fs(v: V) -> @location(0) vec4f { return textureSampleLevel(src, smp, v.uv, 0.0); }`;

export function blit(enc, src, dst, srcLevel, dstLevel, layers) {
  const e = objs[enc].encoder;
  const s = objs[src], d = objs[dst];
  let pipeline = blitPipelines.get(d.format);
  if (!pipeline) {
    const module = device.createShaderModule({code: BLIT_WGSL});
    pipeline = device.createRenderPipeline({layout: 'auto', vertex: {module, entryPoint: 'vs'},
      fragment: {module, entryPoint: 'fs', targets: [{format: d.format}]}});
    blitPipelines.set(d.format, pipeline);
  }
  blitSampler ??= device.createSampler({magFilter: 'linear', minFilter: 'linear'});
  for (let layer = 0; layer < layers; layer++) {
    const one = (t, mip) => t.createView({dimension: '2d', baseMipLevel: mip, mipLevelCount: 1,
      baseArrayLayer: layer, arrayLayerCount: 1});
    const group = device.createBindGroup({layout: pipeline.getBindGroupLayout(0),
      entries: [{binding: 0, resource: one(s, srcLevel)}, {binding: 1, resource: blitSampler}]});
    const pass = e.beginRenderPass({colorAttachments: [{view: one(d, dstLevel), loadOp: 'clear',
      clearValue: [0, 0, 0, 0], storeOp: 'store'}]});
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, group);
    pass.draw(3);
    pass.end();
  }
}

// ---- Compute ----

export function dispatch(enc, pipelineId, group, push, x, y, z, indirect, indirectOffset) {
  const e = objs[enc];
  const p = objs[pipelineId];
  const pass = e.encoder.beginComputePass();
  pass.setPipeline(p.pipeline);
  if (p.userGroups > 0) pass.setBindGroup(0, objs[group]);
  if (p.pushGroup >= 0) {
    const block = new Uint8Array(SLOT);
    block.set(push.subarray(0, SLOT));
    const [g, offset] = e.push(block);
    pass.setBindGroup(p.pushGroup, g, [offset]);
  }
  if (indirect) pass.dispatchWorkgroupsIndirect(objs[indirect], indirectOffset);
  else pass.dispatchWorkgroups(x, y, z);
  pass.end();
}

// ---- Render passes ----

class Pass {
  constructor(encoder, pass, colors, depth) {
    this.encoder = encoder;
    this.pass = pass;
    this.colors = colors;
    this.depth = depth;
    this.pipeline = null;
    this.userGroups = [];
    this.bound = [];
    this.push = new Uint8Array(SLOT);
    this.pushDirty = true;
    this.pushSlot = null;
    this.scissor = null;
  }

  beforeDraw() {
    const p = this.pipeline;
    for (let i = 0; i < p.userGroups; i++) {
      const g = this.userGroups[i];
      if (g && this.bound[i] !== g) { this.pass.setBindGroup(i, objs[g]); this.bound[i] = g; }
    }
    if (p.pushGroup >= 0) {
      if (this.pushDirty || !this.pushSlot) {
        this.pushSlot = this.encoder.push(this.push);
        this.pushDirty = false;
        this.bound[p.pushGroup] = null;
      }
      if (this.bound[p.pushGroup] !== this.pushSlot) {
        this.pass.setBindGroup(p.pushGroup, this.pushSlot[0], [this.pushSlot[1]]);
        this.bound[p.pushGroup] = this.pushSlot;
      }
    }
  }
}

/// `colors` holds [view id, format, load (0 clear / 1 load), store (0/1), r, g, b, a]
/// per target; a view id of 0 is an unused location. `depth` is [view id,
/// load, store, value], or empty.
export function beginRenderPass(enc, colors, depth, width, height) {
  const e = objs[enc];
  const colorAttachments = [];
  const formats = [];
  for (let i = 0; i < colors.length; i += 8) {
    if (!colors[i]) { colorAttachments.push(null); formats.push(null); continue; }
    colorAttachments.push({view: objs[colors[i]], loadOp: colors[i + 2] ? 'load' : 'clear',
      storeOp: colors[i + 3] ? 'store' : 'discard',
      clearValue: [colors[i + 4], colors[i + 5], colors[i + 6], colors[i + 7]]});
    formats.push(colors[i + 1]);
  }
  const desc = {colorAttachments};
  if (depth.length) {
    desc.depthStencilAttachment = {view: objs[depth[0]], depthLoadOp: depth[1] ? 'load' : 'clear',
      depthStoreOp: depth[2] ? 'store' : 'discard', depthClearValue: depth[3]};
  }
  const pass = e.encoder.beginRenderPass(desc);
  const p = new Pass(e, pass, formats, depth.length > 0);
  p.scissor = [0, 0, width, height];
  return put(p);
}

export function endPass(id) { objs[id].pass.end(); objs[id] = null; freeIds.push(id); }

export function setPipeline(id, pipelineId) {
  const p = objs[id];
  p.pipeline = objs[pipelineId];
  p.pass.setPipeline(p.pipeline.pipeline);
}

export function setBindGroup(id, index, group) {
  const p = objs[id];
  p.userGroups[index] = group;
  p.pass.setBindGroup(index, objs[group]);
  p.bound[index] = group;
}

export function setPush(id, offset, bytes) {
  const p = objs[id];
  p.push.set(bytes, offset);
  p.pushDirty = true;
}

export function setVertexBuffer(id, slot, buffer) { objs[id].pass.setVertexBuffer(slot, objs[buffer]); }
export function setIndexBuffer(id, buffer) { objs[id].pass.setIndexBuffer(objs[buffer], 'uint32'); }
export function setViewport(id, x, y, w, h) { objs[id].pass.setViewport(x, y, w, h, 0, 1); }
export function setScissor(id, x, y, w, h) { const p = objs[id]; p.scissor = [x, y, w, h]; p.pass.setScissorRect(x, y, w, h); }

export function draw(id, vertices, instances, firstVertex, firstInstance) {
  const p = objs[id];
  p.beforeDraw();
  p.pass.draw(vertices, instances, firstVertex, firstInstance);
}

export function drawIndexed(id, indices, instances, firstIndex, baseVertex, firstInstance) {
  const p = objs[id];
  p.beforeDraw();
  p.pass.drawIndexed(indices, instances, firstIndex, baseVertex, firstInstance);
}

export function drawIndexedIndirect(id, buffer, offset, count, stride) {
  const p = objs[id];
  p.beforeDraw();
  for (let i = 0; i < count; i++) p.pass.drawIndexedIndirect(objs[buffer], offset + i * stride);
}

const clearPipelines = new Map();
const CLEAR_WGSL = `
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}
struct D { @builtin(frag_depth) depth: f32 };
@fragment fn depth() -> D { return D(1.0); }
@fragment fn color() -> @location(0) vec4f { return vec4f(1.0); }`;

/// Clears inside `rect` mid-pass, as Vulkan's vkCmdClearAttachments does:
/// a scissored draw, after which the pass's own pipeline is set again.
export function clearRect(id, depthOnlyClear, x, y, w, h, r, g, b, a) {
  const p = objs[id];
  const key = (depthOnlyClear ? 'd' : 'c') + p.colors.join(',') + (p.depth ? '+z' : '');
  let pipeline = clearPipelines.get(key);
  if (!pipeline) {
    const module = device.createShaderModule({code: CLEAR_WGSL});
    const targets = p.colors.map((f, i) => f && (depthOnlyClear || i > 0
      ? {format: f, writeMask: 0}
      : {format: f, blend: {color: {srcFactor: 'constant', dstFactor: 'zero'},
          alpha: {srcFactor: 'constant', dstFactor: 'zero'}}}));
    pipeline = device.createRenderPipeline({layout: device.createPipelineLayout({bindGroupLayouts: []}),
      vertex: {module, entryPoint: 'vs'},
      fragment: {module, entryPoint: depthOnlyClear ? 'depth' : 'color', targets},
      ...(p.depth ? {depthStencil: {format: 'depth32float', depthWriteEnabled: depthOnlyClear,
        depthCompare: 'always'}} : {})});
    clearPipelines.set(key, pipeline);
  }
  p.pass.setPipeline(pipeline);
  if (!depthOnlyClear) p.pass.setBlendConstant([r, g, b, a]);
  p.pass.setScissorRect(x, y, w, h);
  p.pass.draw(3);
  if (p.scissor) p.pass.setScissorRect(...p.scissor);
  if (p.pipeline) p.pass.setPipeline(p.pipeline.pipeline);
}

// WebGPU device and canvas lifetime for the Rust render hardware interface.
// Only request Pomme's optional features when the adapter offers them.
const OPTIONAL_FEATURES = ['depth-clip-control', 'timestamp-query', 'shader-f16',
  'indirect-first-instance', 'float32-filterable', 'rg11b10ufloat-renderable',
  'subgroups', 'texture-formats-tier1'];

export class BrowserGpuContext {
  static async create(canvas) {
    const gpu = navigator.gpu;
    if (!gpu) throw new Error('WebGPU is unavailable in this browser');
    const adapter = await gpu.requestAdapter({powerPreference:'high-performance'});
    if (!adapter) throw new Error('No WebGPU adapter is available');
    const requested = OPTIONAL_FEATURES.filter(feature => adapter.features.has(feature));
    const requiredLimits = Object.fromEntries([
      'maxTextureDimension2D', 'maxTextureArrayLayers', 'maxBufferSize',
      'maxStorageBufferBindingSize', 'maxStorageBuffersPerShaderStage',
      'maxStorageTexturesPerShaderStage', 'maxColorAttachments',
      'maxColorAttachmentBytesPerSample',
    ].map(key => [key, adapter.limits[key]]).filter(([,v]) => Number.isFinite(v)));
    const device = await adapter.requestDevice({requiredFeatures:requested, requiredLimits});
    const surface = canvas.getContext('webgpu');
    if (!surface) {
      device.destroy();
      throw new Error('Could not create a WebGPU canvas surface');
    }
    return new BrowserGpuContext(canvas, surface, device, gpu.getPreferredCanvasFormat(), adapter.info);
  }

  constructor(canvas, surface, device, format, adapterInfo) {
    this.canvas = canvas;
    this.surface = surface;
    this.device = device;
    this.format = format;
    this.adapterInfo = adapterInfo;
    this.lossReason = null;
    device.lost.then(info => { this.lossReason = info.message || info.reason; });
    this.resize(canvas.width, canvas.height);
  }

  resize(width, height) {
    const limit = this.device.limits.maxTextureDimension2D;
    this.canvas.width = Math.min(limit, Math.max(1, width));
    this.canvas.height = Math.min(limit, Math.max(1, height));
    this.surface.configure({device:this.device, format:this.format,
      alphaMode:'opaque', usage:GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC});
  }

  acquire() {
    if (this.lossReason) throw new Error('WebGPU device lost: ' + this.lossReason);
    return this.surface.getCurrentTexture();
  }

  async shader(label, source) {
    const module = this.device.createShaderModule({label, code:source});
    const {messages} = await module.getCompilationInfo();
    const errors = messages.filter(message => message.type === 'error');
    if (errors.length) {
      throw new Error(label + ': ' + errors.map(e => e.lineNum + ': ' + e.message).join('\n'));
    }
    return module;
  }

  destroy() {
    this.surface.unconfigure();
    this.device.destroy();
  }
}

// Methods called by the Rust hardware interface. Buffer sizes are rounded up
// for WebGPU's four-byte uploads/copies, without changing their logical size.
BrowserGpuContext.prototype.createBuffer = function(label, size, flags, memory) {
  if (!Number.isSafeInteger(size) || size < 0) throw new RangeError('Invalid buffer size');
  const names = ['VERTEX','INDEX','UNIFORM','STORAGE','INDIRECT','COPY_SRC','COPY_DST'];
  let usage = names.reduce((out, name, bit) => flags & (1 << bit) ? out | GPUBufferUsage[name] : out, 0);
  if (memory === 0) usage |= GPUBufferUsage.COPY_DST;
  if (memory === 2) {
    if ((usage & ~GPUBufferUsage.COPY_DST) !== 0) throw new Error('Readback buffer has non-copy usage');
    usage |= GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST;
  }
  return this.device.createBuffer({label, size:Math.max(4,Math.ceil(size/4)*4), usage});
};

BrowserGpuContext.prototype.writeBuffer = function(buffer, offset, bytes) {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset % 4 !== 0) {
    throw new RangeError('Buffer upload offset must be a multiple of four');
  }
  if (!bytes.byteLength) return;
  if (bytes.byteLength % 4 !== 0) {
    const padded = new Uint8Array(Math.ceil(bytes.byteLength/4)*4);
    padded.set(bytes);
    bytes = padded;
  }
  this.device.queue.writeBuffer(buffer, offset, bytes);
};

BrowserGpuContext.prototype.createTexture = function(label, width, height, layers, levels, format, flags) {
  const names = ['COPY_SRC','COPY_DST','TEXTURE_BINDING','RENDER_ATTACHMENT'];
  const usage = names.reduce((out, name, bit) => flags & (1 << bit) ? out | GPUTextureUsage[name] : out, 0);
  return this.device.createTexture({label,size:[width,height,layers],mipLevelCount:levels,format,usage});
};

BrowserGpuContext.prototype.createTextureView = function(texture, dimension, firstMip, levels, firstLayer, layers) {
  return texture.createView({dimension,baseMipLevel:firstMip,mipLevelCount:levels,
    baseArrayLayer:firstLayer,arrayLayerCount:layers});
};

BrowserGpuContext.prototype.createSampler = function(filter, address, mipmap, maxLod, compare) {
  return this.device.createSampler({magFilter:filter,minFilter:filter,mipmapFilter:mipmap,
    addressModeU:address,addressModeV:address,addressModeW:address,lodMaxClamp:maxLod,
    ...(compare ? {compare} : {})});
};

// The shareable descriptor comes from the Rust renderer; shader modules and
// an optional explicit layout are GPU objects and aren't serialized.
BrowserGpuContext.prototype.createRenderPipeline = async function(json, vertex, fragment, layout) {
  const desc = JSON.parse(json);
  desc.layout = layout ?? 'auto';
  desc.vertex.module = vertex;
  if (desc.fragment) desc.fragment.module = fragment;
  if (desc.primitive.unclippedDepth && !this.device.features.has('depth-clip-control')) {
    delete desc.primitive.unclippedDepth;
  }
  return this.device.createRenderPipelineAsync(desc);
};

BrowserGpuContext.prototype.beginCommands = function(label) {
  return new BrowserCommands(this.device, label);
};

export class BrowserCommands {
  constructor(device, label) {
    this.device = device;
    this.encoder = device.createCommandEncoder({label});
  }

  beginRenderPass(colors, depth, width, height) {
    const pass = this.encoder.beginRenderPass({
      colorAttachments: colors, ...(depth == null ? {} : {depthStencilAttachment: depth}),
    });
    pass.setViewport(0, 0, width, height, 0, 1);
    pass.setScissorRect(0, 0, width, height);
    return new BrowserPass(pass);
  }

  beginComputePass() {
    return new BrowserPass(this.encoder.beginComputePass());
  }

  copyBuffer(src, srcOffset, dst, dstOffset, size) {
    this.encoder.copyBufferToBuffer(src, srcOffset, dst, dstOffset, size);
  }

  copyTexturePixel(texture, x, y, buffer) {
    this.encoder.copyTextureToBuffer({texture,origin:[x,y,0]}, {buffer,bytesPerRow:256}, [1,1,1]);
  }

  submit() {
    this.device.queue.submit([this.encoder.finish()]);
  }
}

export class BrowserPass {
  constructor(pass) { this.pass = pass; }
  pipeline(pipeline) { this.pass.setPipeline(pipeline); }
  vertexBuffer(slot, buffer, offset) { this.pass.setVertexBuffer(slot, buffer, offset); }
  indexBuffer(buffer, format, offset) { this.pass.setIndexBuffer(buffer, format, offset); }
  bind(slot, group) { this.pass.setBindGroup(slot, group); }
  scissor(x, y, width, height) { this.pass.setScissorRect(x, y, width, height); }
  draw(vertices, instances, firstVertex, firstInstance) {
    this.pass.draw(vertices, instances, firstVertex, firstInstance);
  }
  drawIndexed(indices, instances, firstIndex, baseVertex, firstInstance) {
    this.pass.drawIndexed(indices, instances, firstIndex, baseVertex, firstInstance);
  }
  drawIndexedIndirect(buffer, offset) { this.pass.drawIndexedIndirect(buffer, offset); }
  dispatch(x, y, z) { this.pass.dispatchWorkgroups(x, y, z); }
  end() { this.pass.end(); }
}

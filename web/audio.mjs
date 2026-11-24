// Web Audio backend for the same Rust mixer and Ogg decoder used on desktop.
export class PommeAudioContext {
  static create() { return new PommeAudioContext(new AudioContext()); }

  constructor(context) {
    this.context = context;
    this.unlock = new AbortController();
    const resume = () => {
      if (context.state === 'running') this.unlock.abort();
      else void context.resume().then(() => this.unlock.abort()).catch(() => {});
    };
    for (const event of ['pointerdown','keydown','touchend']) {
      globalThis.addEventListener(event,resume,{passive:true,signal:this.unlock.signal});
    }
  }

  listener(position, forward, up) {
    const listener = this.context.listener;
    for (const [name, values] of [['position',position],['forward',forward],['up',up]]) {
      for (const [i, axis] of ['X','Y','Z'].entries()) listener[name+axis].value = values[i];
    }
  }

  buffer(channels, rate, pcm) {
    if (![1,2].includes(channels) || !pcm.length || pcm.length % channels) {
      throw new RangeError('Invalid PCM channel count or frame length');
    }
    const buffer=this.context.createBuffer(channels,pcm.length/channels,rate);
    for (let channel=0;channel<channels;channel++) {
      const output=buffer.getChannelData(channel);
      for (let i=0;i<output.length;i++) output[i]=pcm[i*channels+channel]/32768;
    }
    return buffer;
  }

  source() { return new PommeAudioSource(this.context); }

  close() {
    this.unlock.abort();
    void this.context.close().catch(()=>{});
  }
}

export class PommeAudioSource {
  constructor(context) {
    this.context=context;
    this.gain=context.createGain();
    this.gain.connect(context.destination);
    this.nodes=new Set();
    this.pending=[];
    this.processed=0;
    this.at=0;
    this.started=false;
    this.pitch=1;
  }

  configure(gain,pitch,position,relative,looping,distance) {
    this.setGain(gain);
    this.pitch=pitch;
    this.looping=looping;
    if (!relative && distance>0) {
      this.panner=this.context.createPanner();
      this.panner.distanceModel='linear';
      this.panner.refDistance=0;
      this.panner.maxDistance=distance;
      this.panner.rolloffFactor=1;
      this.panner.connect(this.gain);
      this.position(position);
    }
  }

  setGain(gain) { this.gain.gain.value=gain; }

  position(position) {
    if (!this.panner) return;
    for (const [i,axis] of ['X','Y','Z'].entries()) this.panner['position'+axis].value=position[i];
  }

  attach(buffer) { this.staticBuffer=buffer; }

  queue(buffer) {
    if (this.started) this.schedule(buffer,false);
    else this.pending.push(buffer);
  }

  schedule(buffer, staticSound) {
    const node=this.context.createBufferSource();
    node.buffer=buffer;
    node.playbackRate.value=this.pitch;
    node.loop=staticSound && this.looping;
    node.connect(this.panner ?? this.gain);
    const at=Math.max(this.context.currentTime,this.at);
    this.nodes.add(node);
    node.onended=()=>{
      this.nodes.delete(node);
      node.disconnect();
      if (!staticSound) this.processed++;
    };
    node.start(at);
    this.at=at+buffer.duration/this.pitch;
  }

  play() {
    if (this.started) return;
    this.started=true;
    if (this.staticBuffer) this.schedule(this.staticBuffer,true);
    for (const buffer of this.pending) this.schedule(buffer,false);
    this.pending=[];
  }

  takeProcessed() {
    const count=this.processed;
    this.processed=0;
    return count;
  }

  state() {
    if (!this.started) return 0;
    if (!this.nodes.size) return 3;
    return this.context.state==='running' ? 1 : 2;
  }

  stop() {
    for (const node of this.nodes) {
      node.onended=null;
      try { node.stop(); } catch {}
      node.disconnect();
    }
    this.nodes.clear();
    this.pending=[];
    this.processed=0;
    this.at=0;
    this.started=false;
  }

  close() {
    this.stop();
    this.panner?.disconnect();
    this.gain.disconnect();
  }
}

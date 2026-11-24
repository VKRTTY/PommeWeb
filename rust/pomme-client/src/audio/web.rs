//! Web Audio bindings preserving the shared sound engine's source interface.
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Float32Array, Int16Array};
use wasm_bindgen::prelude::*;

use super::decoder::PcmFormat;

#[wasm_bindgen(raw_module = "/audio.mjs")]
extern "C" {
    type PommeAudioContext;
    type PommeAudioSource;

    #[wasm_bindgen(static_method_of = PommeAudioContext, catch)]
    fn create() -> Result<PommeAudioContext, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn listener(this: &PommeAudioContext, position: &Float32Array, forward: &Float32Array,
        up: &Float32Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn buffer(this: &PommeAudioContext, channels: u16, rate: u32, pcm: &Int16Array)
        -> Result<JsValue, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn source(this: &PommeAudioContext) -> Result<PommeAudioSource, JsValue>;
    #[wasm_bindgen(method, js_name = close)]
    fn close_context(this: &PommeAudioContext);

    #[wasm_bindgen(method, catch)]
    fn configure(this: &PommeAudioSource, gain: f32, pitch: f32, position: &Float32Array,
        relative: bool, looping: bool, distance: f32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = setGain)]
    fn gain(this: &PommeAudioSource, value: f32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn position(this: &PommeAudioSource, position: &Float32Array) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn attach(this: &PommeAudioSource, buffer: &JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn queue(this: &PommeAudioSource, buffer: &JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(method, js_name = takeProcessed)]
    fn take_processed(this: &PommeAudioSource) -> usize;
    #[wasm_bindgen(method, catch)]
    fn play(this: &PommeAudioSource) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn stop(this: &PommeAudioSource);
    #[wasm_bindgen(method, js_name = close)]
    fn close_source(this: &PommeAudioSource);
    #[wasm_bindgen(method)]
    fn state(this: &PommeAudioSource) -> u8;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceState {
    Initial,
    Playing,
    Paused,
    Stopped,
}

struct AudioContext(PommeAudioContext);

impl Drop for AudioContext {
    fn drop(&mut self) {
        self.0.close_context();
    }
}

pub(super) struct Context {
    audio: Rc<AudioContext>,
    pub static_source_limit: usize,
    pub streaming_source_limit: usize,
}

pub(super) struct Buffer {
    audio: Rc<AudioContext>,
    raw: JsValue,
}

pub(super) struct Source {
    audio: Rc<AudioContext>,
    raw: PommeAudioSource,
}

fn reason(error: JsValue) -> String {
    format!("Web Audio: {error:?}")
}

impl Context {
    pub fn open_default(_use_hrtf: bool) -> Result<Self, String> {
        Ok(Self {
            audio: Rc::new(AudioContext(PommeAudioContext::create().map_err(reason)?)),
            static_source_limit: 64,
            streaming_source_limit: 8,
        })
    }

    pub fn set_listener(&self, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) -> Result<(), String> {
        self.audio.0.listener(
            &Float32Array::from(position.as_slice()),
            &Float32Array::from(forward.as_slice()),
            &Float32Array::from(up.as_slice()),
        ).map_err(reason)
    }

    pub fn create_source(&self) -> Result<Source, String> {
        Ok(Source { audio: Rc::clone(&self.audio), raw: self.audio.0.source().map_err(reason)? })
    }

    pub fn create_buffer(&self, format: PcmFormat, samples: &[i16]) -> Result<Buffer, String> {
        let raw = self.audio.0.buffer(format.channels, format.sample_rate, &Int16Array::from(samples))
            .map_err(reason)?;
        Ok(Buffer { audio: Rc::clone(&self.audio), raw })
    }
}

impl Source {
    pub fn configure(&self, gain: f32, pitch: f32, position: [f32; 3], relative: bool,
        looping: bool, attenuation_distance: Option<f32>) -> Result<(), String> {
        self.raw.configure(gain, pitch, &Float32Array::from(position.as_slice()),
            relative, looping, attenuation_distance.unwrap_or(0.0)).map_err(reason)
    }

    pub fn set_gain(&self, gain: f32) -> Result<(), String> { self.raw.gain(gain).map_err(reason) }

    pub fn set_position(&self, position: [f32; 3]) -> Result<(), String> {
        self.raw.position(&Float32Array::from(position.as_slice())).map_err(reason)
    }

    fn check_context(&self, buffer: &Buffer) -> Result<(), String> {
        if Rc::ptr_eq(&self.audio, &buffer.audio) { Ok(()) }
        else { Err("Mismatched Web Audio buffer context".into()) }
    }

    pub fn attach_static(&self, buffer: &Buffer) -> Result<(), String> {
        self.check_context(buffer)?;
        self.raw.attach(&buffer.raw).map_err(reason)
    }

    pub fn queue_buffer(&self, buffer: &Buffer) -> Result<(), String> {
        self.check_context(buffer)?;
        self.raw.queue(&buffer.raw).map_err(reason)
    }

    pub fn remove_processed(&self, queued: &mut VecDeque<Buffer>) -> Result<usize, String> {
        let count = self.raw.take_processed();
        if count > queued.len() { return Err("Web Audio completed an untracked buffer".into()); }
        queued.drain(..count);
        Ok(count)
    }

    pub fn clear_queue(&self, queued: &mut VecDeque<Buffer>) -> Result<(), String> {
        self.stop()?;
        queued.clear();
        Ok(())
    }

    pub fn play(&self) -> Result<(), String> { self.raw.play().map_err(reason) }
    pub fn stop(&self) -> Result<(), String> { self.raw.stop(); Ok(()) }
    pub fn state(&self) -> Result<SourceState, String> {
        Ok(match self.raw.state() {
            0 => SourceState::Initial,
            1 => SourceState::Playing,
            2 => SourceState::Paused,
            _ => SourceState::Stopped,
        })
    }
}

impl Drop for Source {
    fn drop(&mut self) { self.raw.close_source(); }
}

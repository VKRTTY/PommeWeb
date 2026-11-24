//! Browser relay byte stream. The Rust connection still owns Minecraft framing,
//! encryption, compression, and protocol phase changes.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, ready};

use js_sys::{Promise, Uint8Array};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(raw_module = "/transport.mjs")]
extern "C" {
    type RelayTransport;

    #[wasm_bindgen(static_method_of = RelayTransport, catch)]
    fn connect(host: &str, port: u16) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn read(this: &RelayTransport, limit: u32) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn write(this: &RelayTransport, bytes: &Uint8Array) -> Result<(), JsValue>;

    #[wasm_bindgen(method)]
    fn close(this: &RelayTransport);
}

struct Shared(RelayTransport);

impl Drop for Shared {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub struct BrowserReader {
    socket: Rc<Shared>,
    pending: Option<Pin<Box<JsFuture>>>,
    // Retain bytes if a pending read is resumed with a smaller ReadBuf.
    remaining: Vec<u8>,
    offset: usize,
}

pub struct BrowserWriter(Rc<Shared>);

pub async fn connect(host: &str, port: u16) -> io::Result<(BrowserReader, BrowserWriter)> {
    let promise = RelayTransport::connect(host, port).map_err(relay_error)?;
    let socket = JsFuture::from(promise).await.map_err(relay_error)?;
    let socket = Rc::new(Shared(socket.unchecked_into()));
    Ok((
        BrowserReader {
            socket: Rc::clone(&socket),
            pending: None,
            remaining: Vec::new(),
            offset: 0,
        },
        BrowserWriter(socket),
    ))
}

fn relay_error(error: JsValue) -> io::Error {
    io::Error::other(format!("Pomme Relay: {error:?}"))
}

impl AsyncRead for BrowserReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.offset == this.remaining.len() {
            if this.pending.is_none() {
                let limit = buf.remaining().min(u32::MAX as usize) as u32;
                let promise = this.socket.0.read(limit).map_err(relay_error)?;
                this.pending = Some(Box::pin(JsFuture::from(promise)));
            }
            let result = ready!(this.pending.as_mut().unwrap().as_mut().poll(cx));
            this.pending = None;
            let value = result.map_err(relay_error)?;
            if value.is_null() {
                return Poll::Ready(Ok(()));
            }
            this.remaining = value.dyn_into::<Uint8Array>().map_err(relay_error)?.to_vec();
            this.offset = 0;
        }
        let count = buf.remaining().min(this.remaining.len() - this.offset);
        buf.put_slice(&this.remaining[this.offset..this.offset + count]);
        this.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for BrowserWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.0.write(&Uint8Array::from(bytes)).map_err(relay_error)?;
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.0.close();
        Poll::Ready(Ok(()))
    }
}

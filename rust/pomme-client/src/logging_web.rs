//! Browser logs go directly to DevTools, not to a native log file/thread.
use std::io::{self, Write};
use std::path::Path;

use tracing_subscriber::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn console_log(message: &str);

    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

#[derive(Default)]
struct ConsoleLine(Vec<u8>);

impl Write for ConsoleLine {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.0.is_empty() {
            console_log(String::from_utf8_lossy(&self.0).trim_end());
            self.0.clear();
        }
        Ok(())
    }
}

impl Drop for ConsoleLine {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub fn init(_log_dir: &Path) {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer()
            .without_time()
            .with_ansi(false)
            .with_writer(ConsoleLine::default)
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .try_init();
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| console_error(&info.to_string())));
}

# PommeWeb

The browser port of [Pomme](https://github.com/PommeMC/Client), the Rust
Minecraft client: everything that lets it run in a web browser on WebGPU.
This repo holds only the web code; it builds against a Pomme checkout.

![Port progress](docs/progress.svg)

**Status:** in progress. The game does not run in a browser yet. Shaders,
server connections, audio and the task runtime are ported; the WebGPU
graphics backend is being written, and the loading screen and title menu are
the first goal.

## What's here

- [`web/`](web) — JavaScript the page runs: [`webgpu.mjs`](web/webgpu.mjs)
  (graphics), [`transport.mjs`](web/transport.mjs) (server connections
  through the relay), [`audio.mjs`](web/audio.mjs) (Web Audio),
  [`asset-storage.mjs`](web/asset-storage.mjs) (game files in IndexedDB), and
  the page, [`index.html`](web/index.html).
- [`rust/`](rust) — the new browser-only Rust files, at the paths they take in
  Pomme: the shader converter, the WebGPU backend, the relay connection, Web
  Audio, console logging, the browser task runtime, and the installer's
  browser storage.
- [`patches/pomme.patch`](patches/pomme.patch) — the port's edits to Pomme's
  own files (mostly `cfg(target_arch = "wasm32")` switches), against the
  Pomme commit in [`patches/BASE`](patches/BASE).
- [`docs/`](docs) — the progress map and its data.

Almost all of the game (menus, world rendering, protocol, physics) is Pomme's
own Rust, compiled to WebAssembly unchanged. Only the places it touches the
operating system get a browser version:

| The game needs | Desktop | Browser |
|---|---|---|
| Graphics | Vulkan | WebGPU |
| Shaders | SPIR-V | WGSL, converted at build time |
| Server connections | TCP | WebSocket to `pomme-relay`, which dials the server |
| Sound | OpenAL | Web Audio |
| Background tasks | Tokio | the page's event loop |
| Game files | disk | IndexedDB |
| Logs | log file | browser console |

## Build and run

Needs a Pomme checkout at commit `patches/BASE`, the pinned Rust nightly with
the `wasm32-unknown-unknown` target, and `wasm-bindgen` 0.2.121
(`cargo install wasm-bindgen-cli --version 0.2.121`).

    cp -R rust/. /path/to/pomme/
    git -C /path/to/pomme apply "$PWD/patches/pomme.patch"
    POMME_REPO=/path/to/pomme ./build.sh
    cd /path/to/pomme && cargo run -p pomme-relay -- --dir /path/to/PommeWeb/dist

Then open http://127.0.0.1:8080 in a browser with WebGPU (Chrome or Edge).

`sync.sh` copies the Rust side back out of a working Pomme checkout, and
`python3 docs/progress.py` redraws the progress map from
[`docs/progress.json`](docs/progress.json).

## License

GPL-3.0-or-later, as Pomme is.

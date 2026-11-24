<div align="center">

# PommeWeb

**[Pomme](https://github.com/PommeMC/Client), the Rust Minecraft client, running in your browser.**

![WebGPU](https://img.shields.io/badge/graphics-WebGPU-005A9C?style=flat-square)
![WebAssembly](https://img.shields.io/badge/runs%20on-WebAssembly-654FF0?style=flat-square&logo=webassembly&logoColor=white)
![Rust](https://img.shields.io/badge/written%20in-Rust-B7410E?style=flat-square&logo=rust&logoColor=white)
![Status](https://img.shields.io/badge/status-playable-2ea043?style=flat-square)
![License](https://img.shields.io/badge/license-GPL--3.0-2ea043?style=flat-square)

</div>

<br>

![Port progress](docs/progress.svg)

> [!NOTE]
> **It plays in the browser.** Pomme's own renderer draws the menus and the
> world in Chrome through WebGPU, joins servers through `pomme-relay`, and
> plays local singleplayer worlds on the same SteelMC server as desktop.
> The **Meteor Client** port is under way (see below). Microsoft sign-in
> and distant terrain are still in progress.

## Screenshots

<table>
<tr>
<td colspan="2"><img src="docs/screenshots/singleplayer-gameplay.png" alt="A singleplayer world played in Chrome"></td>
</tr>
<tr>
<td><img src="docs/screenshots/title-menu.png" alt="Title menu with the panorama"></td>
<td><img src="docs/screenshots/gameplay-third-person.png" alt="Third-person gameplay"></td>
</tr>
<tr>
<td><img src="docs/screenshots/loading-mojang.png" alt="Loading screen"></td>
<td><img src="docs/screenshots/loading-install.png" alt="First-run install progress"></td>
</tr>
<tr>
<td><img src="docs/screenshots/menu-03-multiplayer.png" alt="Multiplayer"></td>
<td><img src="docs/screenshots/menu-04-options.png" alt="Options"></td>
</tr>
</table>

## Features

| | |
|---|---|
| **Renderer** | Pomme's own, on WebGPU: world, entities, HUD, menus with background blur |
| **Play** | Local singleplayer worlds, and any offline-mode Java server through the relay |
| **Loading** | Vanilla's red Mojang Studios screen in the Minecraft font; the game downloads once and starts in seconds after |
| **Storage** | Game files, worlds and settings kept in the browser (IndexedDB) |
| **Sound** | Web Audio; sounds download the first time they play |
| **Meteor Client** | Module system, click GUI and HUD in Meteor's theme (below) |

## Meteor Client port

A port of [Meteor Client](https://github.com/MeteorDevelopment/meteor-client)
(GPL-3.0, by MeteorDevelopment) to Pomme. PommeWeb is GPL-3.0 too, and Meteor's
license and notices stay with the code derived from it.

Press **Right Shift** in a world, or use **Meteor Client** on the pause menu.

- **Same modules, settings and defaults.** All 169 modules (Combat 30, Player
  31, Movement 30, Render 38, World 24, Misc 16) and their 1,407 settings come
  straight from Meteor's source, by `scripts/meteor-extract.py`, so names,
  descriptions, defaults, ranges and options match.
- **Same GUI.** Meteor's theme colors and layout: the tab bar, a draggable
  window per category with the accent header, left-click to toggle,
  right-click for settings, sliders, dropdowns, binds, favorites and tooltips.
  Window positions and every setting persist.
- **Same HUD.** The default Meteor HUD: watermark, FPS, ping, speed, the
  rainbow active-modules list, position, opposite-dimension position and
  rotation.
- **Behavior is ported module by module.** Working now: Fullbright, Zoom,
  Sprint, Auto Walk and Auto Jump. The rest keep their settings but don't act
  yet; the full status is in [HANDOFF.md](HANDOFF.md).

## How it works

Almost all of the game — menus, world rendering, protocol, physics — is
Pomme's own Rust, compiled to WebAssembly unchanged. Only the places where the
game touches the operating system get a browser version:

| The game needs | On desktop | In the browser |
|---|---|---|
| 🎨 Graphics | Vulkan | WebGPU |
| ✨ Shaders | SPIR-V | WGSL, converted at build time |
| 🌐 Server connections | TCP sockets | WebSocket to `pomme-relay`, which dials the server |
| 🔊 Sound | OpenAL | Web Audio |
| ⚙️ Background tasks | Tokio | the page's event loop |
| 📦 Game files | disk | IndexedDB |
| 📝 Logs | log file | browser console |

## What's in this repo

This repo holds only the web code; it builds against a Pomme checkout.

| Path | Contents |
|---|---|
| [`web/`](web) | JavaScript the page runs — [`webgpu.mjs`](web/webgpu.mjs) (graphics), [`transport.mjs`](web/transport.mjs) (server connections), [`audio.mjs`](web/audio.mjs) (sound), [`asset-storage.mjs`](web/asset-storage.mjs) (game files) — and the page, [`index.html`](web/index.html) |
| [`rust/`](rust) | New browser-only Rust files, at the paths they take in Pomme |
| [`patches/pomme.patch`](patches/pomme.patch) | The port's edits to Pomme's own files, against the commit in [`patches/BASE`](patches/BASE) |
| [`docs/`](docs) | The progress map and its data |

## Play

**One command:** `./play.sh --build` rebuilds the page, starts the loopback
relay with integrated SteelMC support, and opens Chrome. Use **Singleplayer**
to create a world or choose **Play Selected World**. Later launches only need
`./play.sh`. World data lives under `.server/worlds`; the browser keeps the
world list for its current origin. Singleplayer requires the local relay and
is not offered as a publicly hosted service. Continuing the port? Start with
[HANDOFF.md](HANDOFF.md).

By hand:

1. Build it and start the relay (below), then open **http://127.0.0.1:8460**
   in Chrome or Edge.
2. The first visit downloads the game files from Mojang (about 50 MB) behind
   the loading screen; later visits start in seconds.
3. **Multiplayer → Direct Connection**, enter the server's address, **Join
   Server**. You play as `Player` for now, so the server must be in offline
   mode (`online-mode=false`) until Microsoft sign-in comes to the browser.
4. To play on a server on your own machine, start the relay with
   `--allow-private-servers`.

## Build and run

You need a Pomme checkout at the commit in `patches/BASE`, the pinned Rust
nightly with the `wasm32-unknown-unknown` target, and `wasm-bindgen` 0.2.121:

```sh
cargo install wasm-bindgen-cli --version 0.2.121
```

Then:

```sh
cp -R rust/. /path/to/pomme/
git -C /path/to/pomme apply "$PWD/patches/pomme.patch"
POMME_REPO=/path/to/pomme ./build.sh
cd /path/to/pomme && cargo run -p pomme-relay --release -- \
  --dir /path/to/PommeWeb/dist --listen 127.0.0.1:8460 --allow-private-servers
```

and open **http://127.0.0.1:8460** in a browser with WebGPU (Chrome or Edge).
For local singleplayer, build the relay with `--features local-worlds` and
pass `--local-worlds /path/to/world-saves` after Cargo's `--`. Neither
`--public-host` nor `--trust-proxy` can be combined with this option.

<details>
<summary>Working on the port</summary>

- `sync.sh` copies the Rust side back out of a working Pomme checkout.
- `python3 docs/progress.py` redraws the progress map from
  [`docs/progress.json`](docs/progress.json).

</details>

## Railway

**[Open PommeWeb on Railway](https://mcweb-production-20e8ff9d.up.railway.app/).**

Deploy this repository as a Dockerfile service. Railway supplies the public
domain and port; the container serves the built browser client and relay.
The hosted relay only connects to public game servers. Local singleplayer
continues to use `./play.sh` on your own machine.

## License

GPL-3.0-or-later, as Pomme is. See [LICENSE](LICENSE).

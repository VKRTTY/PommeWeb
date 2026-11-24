// Shows vanilla's red loading screen while the game downloads, then starts
// the game on the page's canvas.
import init, {start} from './pomme.js';
import {assetFind} from './asset-storage.mjs';

const loading = document.getElementById('loading');
const fill = document.getElementById('fill');
const status = document.getElementById('status');
let font = null, lastText = '';

/// Vanilla's default font: `ascii.png`'s 16x16 grid, each glyph as wide as
/// its rightmost lit column plus one, spaces four.
function loadFont(image) {
  const cell = image.width / 16;
  const canvas = new OffscreenCanvas(image.width, image.height);
  const ctx = canvas.getContext('2d');
  ctx.drawImage(image, 0, 0);
  const pixels = ctx.getImageData(0, 0, image.width, image.height).data;
  const widths = new Array(256).fill(0);
  for (let code = 0; code < 256; code++) {
    const x0 = (code % 16) * cell, y0 = Math.floor(code / 16) * cell;
    let right = -1;
    for (let x = 0; x < cell; x++) {
      for (let y = 0; y < cell; y++) {
        if (pixels[((y0 + y) * image.width + x0 + x) * 4 + 3] > 0) { right = x; break; }
      }
    }
    widths[code] = code === 32 ? 4 : Math.ceil((right + 1) * 8 / cell) + 1;
  }
  return {image, cell, widths};
}

/// `text` centered on the status canvas in the game's font, with vanilla's
/// shadow: a quarter-bright copy one unit down and right.
function drawText(text) {
  lastText = text;
  const dpr = devicePixelRatio || 1;
  const gs = parseFloat(getComputedStyle(loading).getPropertyValue('--gs')) * dpr;
  status.width = Math.round(innerWidth * dpr);
  status.height = Math.round(10 * gs);
  const ctx = status.getContext('2d');
  ctx.imageSmoothingEnabled = false;
  if (!font) return;
  const chars = [...text].map(c => c.charCodeAt(0)).map(c => c < 256 ? c : 63);
  const width = chars.reduce((w, c) => w + font.widths[c], 0) * gs;
  for (const [offset, filter] of [[gs, 'brightness(25%)'], [0, 'none']]) {
    ctx.filter = filter;
    let x = Math.round((status.width - width) / 2) + offset;
    for (const c of chars) {
      const sx = (c % 16) * font.cell, sy = Math.floor(c / 16) * font.cell;
      ctx.drawImage(font.image, sx, sy, font.cell, font.cell, x, offset, 8 * gs, 8 * gs);
      x += font.widths[c] * gs;
    }
  }
}
const say = text => drawText(`Pomme: ${text}`);

// Vanilla's automatic GUI scale: the largest that keeps 320x240 on screen.
const guiScale = () => {
  loading.style.setProperty('--gs',
    `${Math.max(1, Math.floor(Math.min(innerWidth / 320, innerHeight / 240)))}px`);
  drawText(lastText);
};
guiScale();
addEventListener('resize', guiScale);
const setProgress = fraction => { fill.style.width = `${Math.min(1, fraction) * 100}%`; };

// The logo and font are the game's own textures. A visit caches them in
// localStorage once the game files hold them (early.js shows the logo from
// there before the first paint); a first visit looks again while the game
// installs.
const cached = name => { try { return localStorage.getItem(name); } catch { return null; } };
const cache = (name, url) => { try { localStorage.setItem(name, url); } catch {} };
const dataUrl = blob => new Promise(resolve => {
  const reader = new FileReader();
  reader.onload = () => resolve(reader.result);
  reader.readAsDataURL(blob);
});
// Through an image: the page may load images from data URLs, not fetch them.
const useFont = url => new Promise((resolve, reject) => {
  const image = new Image();
  image.onload = () => { font = loadFont(image); drawText(lastText); resolve(); };
  image.onerror = reject;
  image.src = url;
});
let logoShown = !!cached('pomme-logo'), looked = 0;
const cachedFont = cached('pomme-font');
if (cachedFont) useFont(cachedFont).catch(() => {});
async function showTextures(now = false) {
  if ((logoShown && font) || (!now && Date.now() - looked < 2000)) return;
  looked = Date.now();
  const stored = async suffix => {
    const bytes = await assetFind(suffix).catch(() => undefined);
    return bytes && dataUrl(new Blob([bytes], {type: 'image/png'}));
  };
  if (!font) {
    const url = await stored('/assets/minecraft/textures/font/ascii.png');
    if (url) { cache('pomme-font', url); await useFont(url).catch(e => console.warn('font', e)); }
  }
  if (!logoShown) {
    const url = await stored('/textures/gui/title/mojangstudios.png');
    if (url) {
      logoShown = true;
      cache('pomme-logo', url);
      document.documentElement.style.setProperty('--logo', `url(${url})`);
    }
  }
}
say('Downloading the game...');
showTextures();
const lookAgain = setInterval(showTextures, 2000);

/// The service worker must control the page before the game makes requests
/// to Mojang, so a first visit reloads once it's installed.
async function relayRequests() {
  if (!('serviceWorker' in navigator)) return;
  await navigator.serviceWorker.register('./sw.js');
  await navigator.serviceWorker.ready;
  if (!navigator.serviceWorker.controller) {
    location.reload();
    await new Promise(() => {});
  }
}

/// The game's wasm, with the download filling the bar.
async function download(url) {
  const response = await fetch(url);
  const total = Number(response.headers.get('content-length')) || 0;
  if (!total || !response.body) return response.arrayBuffer();
  const bytes = new Uint8Array(total);
  const reader = response.body.getReader();
  let received = 0;
  for (;;) {
    const {done, value} = await reader.read();
    if (done) break;
    bytes.set(value, received);
    received += value.length;
    setProgress(received / total);
    say(`Downloading the game (${(received / 1048576).toFixed(1)}/${(total / 1048576).toFixed(1)} MB)`);
  }
  return bytes;
}

try {
  await relayRequests();
  const wasm = await download(new URL('./pomme_bg.wasm', import.meta.url));
  say('Starting...');
  await init({module_or_path: wasm});
  // The page's screen stays over the canvas until the game is at its menu,
  // showing the game's own progress meanwhile.
  globalThis.pommeSplash = (progress, text) => { setProgress(progress); say(text); showTextures(); };
  globalThis.pommeReady = () => {
    loading.classList.add('done');
    clearInterval(lookAgain);
    // The game files are surely in by now, for the next visit's cache.
    showTextures(true);
  };
  await start(document.getElementById('pomme'));
} catch (e) {
  console.error(e);
  say(`failed to start: ${e?.message ?? e}`);
}

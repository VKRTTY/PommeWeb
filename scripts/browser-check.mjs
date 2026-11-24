// Opens the built page in a real Chrome window (headless Chrome has no GPU
// on macOS), prints its console, and saves screenshots.
//   node scripts/browser-check.mjs <url> <seconds> <screenshot.png> [every-seconds]
//   node scripts/browser-check.mjs <url> - <out-dir> <actions.json>
// An actions file is a list of steps: ["wait", seconds], ["shot", name],
// ["click", x, y] (CSS pixels), ["key", "Escape"], ["type", "text"], ["reload"], ["eval", "js"].
import {spawn} from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const [url, seconds = '20', shot = 'shot.png', every = '0'] = process.argv.slice(2);
const chrome = process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const port = 9300 + Math.floor(Math.random() * 500);
const profile = await fs.mkdtemp(path.join(os.tmpdir(), 'pomme-chrome-'));
const proc = spawn(chrome, [`--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--no-default-browser-check', '--enable-unsafe-webgpu',
  '--window-size=1280,800', url], {stdio: 'ignore'});
const sleep = ms => new Promise(r => setTimeout(r, ms));

let target;
for (let i = 0; i < 100 && !target; i++) {
  try {
    const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    target = pages.find(p => p.type === 'page' && p.url.startsWith(url.split('#')[0]));
  } catch {}
  if (!target) await sleep(200);
}
if (!target) { proc.kill(); throw new Error('page not found'); }

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let id = 0;
const pending = new Map();
ws.onmessage = ({data}) => {
  const m = JSON.parse(data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
  if (m.method === 'Runtime.consoleAPICalled') {
    const text = m.params.args.map(a => a.value ?? a.description ?? '').join(' ');
    console.log(`[${m.params.type}] ${text}`);
  } else if (m.method === 'Runtime.exceptionThrown') {
    const d = m.params.exceptionDetails;
    console.log(`[exception] ${d.exception?.description ?? d.text}`);
  } else if (m.method === 'Log.entryAdded') {
    console.log(`[${m.params.entry.level}] ${m.params.entry.text}`);
  }
};
const send = (method, params = {}) => new Promise(res => {
  const n = ++id;
  pending.set(n, res);
  ws.send(JSON.stringify({id: n, method, params}));
});
await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');
await send('Page.reload', {ignoreCache: true});

const capture = async file => {
  const r = await send('Page.captureScreenshot', {format: 'png'});
  if (r.result?.data) await fs.writeFile(file, Buffer.from(r.result.data, 'base64'));
};
if (seconds === '-') {
  const steps = JSON.parse(await fs.readFile(every, 'utf8'));
  await fs.mkdir(shot, {recursive: true});
  const VK = {Escape: 27, Enter: 13, ' ': 32, Shift: 16, Tab: 9, Backspace: 8};
  const key = async (type, k) => {
    const fkey = /^F(\d+)$/.exec(k);
    const code = k === ' ' ? 'Space' : k === '/' ? 'Slash' : /^[a-z]$/i.test(k) ? `Key${k.toUpperCase()}`
      : /^\d$/.test(k) ? `Digit${k}` : k === '.' ? 'Period' : k === '~' ? 'Backquote' : k;
    const vk = VK[k] ?? (fkey ? 111 + Number(fkey[1]) : k.toUpperCase().charCodeAt(0));
    await send('Input.dispatchKeyEvent', {type, key: k, code, windowsVirtualKeyCode: vk,
      ...(k.length === 1 && type === 'keyDown' ? {text: k} : {})});
  };
  for (const [what, ...args] of steps) {
    if (what === 'wait') await sleep(args[0] * 1000);
    else if (what === 'shot') { await capture(path.join(shot, `${args[0]}.png`)); console.log(`shot: ${args[0]}`); }
    else if (what === 'click') {
      const [x, y] = args;
      await send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
      await sleep(50);
      await send('Input.dispatchMouseEvent', {type: 'mousePressed', x, y, button: 'left', clickCount: 1});
      await sleep(50);
      await send('Input.dispatchMouseEvent', {type: 'mouseReleased', x, y, button: 'left', clickCount: 1});
    } else if (what === 'eval') {
      const r = await send('Runtime.evaluate', {expression: args[0], returnByValue: true, awaitPromise: true});
      console.log(`eval: ${args[0]} => ${JSON.stringify(r.result?.result?.value)}`);
    } else if (what === 'reload') await send('Page.reload', {ignoreCache: false});
    else if (what === 'key') { await key('keyDown', args[0]); await sleep(30); await key('keyUp', args[0]); }
    else if (what === 'hold') { await key('keyDown', args[0]); await sleep(args[1] * 1000); await key('keyUp', args[0]); }
    else if (what === 'type') for (const c of args[0]) { await key('keyDown', c); await key('keyUp', c); await sleep(20); }
    await sleep(100);
  }
  ws.close();
  proc.kill();
  await fs.rm(profile, {recursive: true, force: true}).catch(() => {});
  process.exit(0);
}
const total = Number(seconds) * 1000;
const step = Number(every) * 1000;
let shots = 0;
for (let t = 0; t < total; t += step || total) {
  await sleep(step || total);
  if (step) await capture(shot.replace(/\.png$/, `-${String(++shots).padStart(2, '0')}.png`));
}
await capture(shot);
console.log(`screenshot: ${shot}`);
ws.close();
proc.kill();
await fs.rm(profile, {recursive: true, force: true}).catch(() => {});

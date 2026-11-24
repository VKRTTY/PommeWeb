// Opens the built page in a real Chrome window (headless Chrome has no GPU
// on macOS), prints its console, and saves screenshots.
//   node scripts/browser-check.mjs <url> <seconds> <screenshot.png> [every-seconds]
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

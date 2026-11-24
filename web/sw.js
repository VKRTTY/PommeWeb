// Sends the game's requests to Mojang and Microsoft through the relay
// (pomme-relay's /relay/<host>/<path>): those services allow no cross-origin
// requests, so the page can't call them itself.
const RELAYED = new Set([
  'piston-meta.mojang.com', 'piston-data.mojang.com', 'libraries.minecraft.net',
  'resources.download.minecraft.net', 'textures.minecraft.net', 'api.mojang.com',
  'api.minecraftservices.com', 'sessionserver.mojang.com', 'login.microsoftonline.com',
  'user.auth.xboxlive.com', 'xsts.auth.xboxlive.com',
]);

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', event => event.waitUntil(self.clients.claim()));

self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  if (url.protocol !== 'https:' || !RELAYED.has(url.hostname)) return;
  event.respondWith(relay(event.request, url));
});

async function relay(request, url) {
  const target = `${self.location.origin}/relay/${url.hostname}${url.pathname}${url.search}`;
  const init = {method: request.method, headers: request.headers, redirect: 'manual'};
  if (request.method !== 'GET' && request.method !== 'HEAD') init.body = await request.arrayBuffer();
  return fetch(target, init);
}

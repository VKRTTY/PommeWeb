import {test} from 'node:test';
import assert from 'node:assert/strict';
import {relayEndpoint, RelayTransport} from './transport.mjs';
class FakeSocket extends EventTarget {
  readyState = 0;
  bufferedAmount = 0;
  sent = [];
  constructor(url) {
    super();
    this.url = url;
    FakeSocket.current = this;
    queueMicrotask(() => {
      this.readyState = 1;
      this.dispatchEvent(new Event('open'));
    });
  }
  send(data) { this.sent.push([...data]); }
  close() { this.dispatchEvent(new Event('close')); }
  emit(data) { this.dispatchEvent(new MessageEvent('message', {data})); }
}
test('uses the same-origin relay and rejects path ambiguity', () => {
  assert.equal(relayEndpoint('https://pomme.example/app?old=1#x', 'mc.example', 25565),
    'wss://pomme.example/connect/mc.example/25565');
  for (const host of ['..', 'a/b', 'a%2fb', 'a\\b']) {
    assert.throws(() => relayEndpoint('https://pomme.example', host, 25565));
  }
});
test('preserves packet bytes across WebSocket message and read boundaries', async () => {
  const transport = await RelayTransport.connect('mc.example', 25565, 'http://localhost:5000', FakeSocket);
  const socket = FakeSocket.current;
  assert.equal(socket.binaryType, 'arraybuffer');
  transport.write(new Uint8Array([1, 2]));
  assert.deepEqual(socket.sent, [[1, 2]]);
  const pending = transport.read(2);
  socket.emit(new Uint8Array([3, 4, 5]).buffer);
  assert.deepEqual([...(await pending)], [3, 4]);
  socket.emit(new Uint8Array([6]).buffer);
  socket.close();
  assert.deepEqual([...(await transport.read(8))], [5]);
  assert.deepEqual([...(await transport.read(8))], [6]);
  assert.equal(await transport.read(8), null);
});
test('rejects non-binary messages rather than parsing them as packets', async () => {
  const transport = await RelayTransport.connect('mc.example', 25565, 'http://localhost:5000', FakeSocket);
  const read = transport.read(8);
  FakeSocket.current.emit('unexpected text');
  await assert.rejects(read, /Invalid or oversized/);
});

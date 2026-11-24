// Browser byte transport for Pomme Relay. Packet framing, encryption and
// compression remain the Rust client's responsibility.
const MAX_BUFFERED = 8 * 1024 * 1024;

export function relayEndpoint(page, host, port) {
  const endpoint = new URL(page);
  if (endpoint.protocol !== 'https:' && endpoint.protocol !== 'http:') {
    throw new TypeError('Pomme Relay requires an HTTP(S) origin');
  }
  if (!host || /[\s/\\%?#]/u.test(host) || host === '.' || host === '..' ||
      !Number.isInteger(port) || port < 1024 || port > 65535) {
    throw new TypeError('Invalid Minecraft server address');
  }
  endpoint.protocol = endpoint.protocol === 'https:' ? 'wss:' : 'ws:';
  endpoint.pathname = '/connect/' + encodeURIComponent(host) + '/' + port;
  endpoint.search = '';
  endpoint.hash = '';
  return endpoint.href;
}

export function localWorldEndpoint(page, path) {
  if (!/^\/local-world\/[0-9a-f]{32}\/-?\d+\/\d+\/\d+$/.test(path)) {
    throw new TypeError('Invalid local world endpoint');
  }
  const origin = new URL(page);
  if (!['http:', 'https:'].includes(origin.protocol)) throw new TypeError('Invalid relay origin');
  const url = new URL(path, origin.origin);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.href;
}

export class RelayTransport {
  #socket;
  #queue = [];
  #bytes = 0;
  #waiter;
  #closed = false;
  #error;

  constructor(socket) {
    this.#socket = socket;
    socket.binaryType = 'arraybuffer';
    socket.addEventListener('message', ({data}) => {
      if (this.#closed) return;
      if (!(data instanceof ArrayBuffer) || this.#bytes + data.byteLength > MAX_BUFFERED) {
        this.#finish(new Error('Invalid or oversized relay data'));
        socket.close(1009);
      } else if (data.byteLength) {
        this.#queue.push(new Uint8Array(data));
        this.#bytes += data.byteLength;
        this.#flush();
      }
    });
    socket.addEventListener('close', () => this.#finish());
    socket.addEventListener('error', () => this.#finish(new Error('Relay connection failed')));
  }

  static async connect(host, port, page = globalThis.location.href, Socket = globalThis.WebSocket) {
    return RelayTransport.open(relayEndpoint(page, host, port), Socket);
  }

  static async connectWorld(path) {
    return RelayTransport.open(localWorldEndpoint(globalThis.location.origin, path), globalThis.WebSocket);
  }

  static async open(url, Socket) {
    const socket = new Socket(url);
    const transport = new RelayTransport(socket);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, {once: true});
      socket.addEventListener('error', () => reject(new Error('Relay connection failed')), {once: true});
      socket.addEventListener('close', () => reject(new Error('Relay closed before opening')), {once: true});
    });
    return transport;
  }

  read(limit) {
    if (!Number.isInteger(limit) || limit < 1) throw new RangeError('Invalid read size');
    if (this.#waiter) throw new Error('Concurrent relay reads are not supported');
    return new Promise((resolve, reject) => {
      this.#waiter = {limit, resolve, reject};
      this.#flush();
    });
  }

  write(bytes) {
    if (!(bytes instanceof Uint8Array)) throw new TypeError('Expected packet bytes');
    if (this.#closed || this.#socket.readyState !== 1) throw new Error('Relay is not open');
    if (this.#socket.bufferedAmount + bytes.byteLength > MAX_BUFFERED) {
      throw new Error('Relay write buffer is full');
    }
    this.#socket.send(bytes);
  }

  close() {
    this.#finish();
    this.#socket.close();
  }

  #finish(error) {
    if (this.#closed) return;
    this.#closed = true;
    this.#error = error;
    this.#flush();
  }

  #flush() {
    if (!this.#waiter) return;
    const {limit, resolve, reject} = this.#waiter;
    if (this.#queue.length) {
      const head = this.#queue[0];
      const data = head.subarray(0, limit);
      if (data.length === head.length) this.#queue.shift();
      else this.#queue[0] = head.subarray(data.length);
      this.#bytes -= data.length;
      this.#waiter = undefined;
      resolve(data);
    } else if (this.#closed) {
      this.#waiter = undefined;
      if (this.#error) reject(this.#error);
      else resolve(null);
    }
  }
}

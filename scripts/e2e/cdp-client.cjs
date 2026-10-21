// Minimal zero-dependency CDP (Chrome DevTools Protocol) client over raw TCP.
// Implements just enough of RFC6455 (client-masked text frames, server unmasked)
// to talk to a WebView2/Chromium remote-debugging endpoint.
const net = require('net');
const http = require('http');
const crypto = require('crypto');

function getTargets(port = 9222) {
  return new Promise((resolve, reject) => {
    const req = http.get({ host: '127.0.0.1', port, path: '/json' }, (res) => {
      let body = '';
      res.on('data', (d) => (body += d));
      res.on('end', () => {
        try { resolve(JSON.parse(body)); } catch (e) { reject(e); }
      });
    });
    req.on('error', reject);
  });
}

async function getPageWsUrl(port = 9222, urlHint = '') {
  const targets = await getTargets(port);
  const pages = targets.filter((t) => t.type === 'page');
  if (pages.length === 0) throw new Error('no page target on port ' + port);
  const page = (urlHint && pages.find((p) => p.url.includes(urlHint))) || pages[0];
  return page.webSocketDebuggerUrl;
}

class Cdp {
  constructor() {
    this.sock = null;
    this.buf = Buffer.alloc(0);
    this.handshakeBuf = Buffer.alloc(0);
    this.nextId = 1;
    this.pending = new Map();
    this.eventListeners = new Map();
  }

  connect(wsUrl) {
    return new Promise((resolve, reject) => {
      const m = wsUrl.match(/^ws:\/\/([^:/]+):(\d+)(\/.*)$/);
      if (!m) return reject(new Error('bad ws url: ' + wsUrl));
      const [, host, port, path] = m;
      const key = crypto.randomBytes(16).toString('base64');
      this.sock = net.connect(Number(port), host, () => {
        this.sock.write(
          `GET ${path} HTTP/1.1\r\nHost: ${host}:${port}\r\n` +
          'Upgrade: websocket\r\nConnection: Upgrade\r\n' +
          `Sec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`
        );
      });
      let upgraded = false;
      this.sock.on('data', (chunk) => {
        if (!upgraded) {
          this.handshakeBuf = Buffer.concat([this.handshakeBuf, chunk]);
          const idx = this.handshakeBuf.indexOf('\r\n\r\n');
          if (idx === -1) return;
          const head = this.handshakeBuf.slice(0, idx).toString();
          if (!head.includes('101')) return reject(new Error('upgrade failed: ' + head.split('\r\n')[0]));
          upgraded = true;
          const rest = this.handshakeBuf.slice(idx + 4);
          this.buf = rest;
          resolve();
          if (rest.length) this._drain();
        } else {
          this.buf = Buffer.concat([this.buf, chunk]);
          this._drain();
        }
      });
      this.sock.on('error', reject);
    });
  }

  _drain() {
    for (;;) {
      if (this.buf.length < 2) return;
      const b0 = this.buf[0];
      const b1 = this.buf[1];
      const opcode = b0 & 0x0f;
      let len = b1 & 0x7f;
      let off = 2;
      if (len === 126) {
        if (this.buf.length < 4) return;
        len = this.buf.readUInt16BE(2); off = 4;
      } else if (len === 127) {
        if (this.buf.length < 10) return;
        len = Number(this.buf.readBigUInt64BE(2)); off = 10;
      }
      if (this.buf.length < off + len) return;
      const payload = this.buf.slice(off, off + len);
      this.buf = this.buf.slice(off + len);
      if (opcode === 0x1) {
        let msg;
        try { msg = JSON.parse(payload.toString('utf8')); } catch { continue; }
        if (msg.id && this.pending.has(msg.id)) {
          const { resolve, reject } = this.pending.get(msg.id);
          this.pending.delete(msg.id);
          msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
        } else if (msg.method) {
          const fns = this.eventListeners.get(msg.method);
          if (fns) for (const fn of fns) fn(msg.params);
        }
      } else if (opcode === 0x8) {
        // close
        return;
      } else if (opcode === 0x9) {
        this._sendFrame(0xA, payload); // pong
      }
    }
  }

  _sendFrame(opcode, payload) {
    const mask = crypto.randomBytes(4);
    const len = payload.length;
    let head;
    if (len < 126) {
      head = Buffer.from([0x80 | opcode, 0x80 | len]);
    } else if (len < 65536) {
      head = Buffer.alloc(4);
      head[0] = 0x80 | opcode; head[1] = 0x80 | 126;
      head.writeUInt16BE(len, 2);
    } else {
      head = Buffer.alloc(10);
      head[0] = 0x80 | opcode; head[1] = 0x80 | 127;
      head.writeBigUInt64BE(BigInt(len), 2);
    }
    const masked = Buffer.alloc(len);
    for (let i = 0; i < len; i++) masked[i] = payload[i] ^ mask[i & 3];
    this.sock.write(Buffer.concat([head, mask, masked]));
  }

  send(method, params = {}) {
    const id = this.nextId++;
    const payload = Buffer.from(JSON.stringify({ id, method, params }), 'utf8');
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this._sendFrame(0x1, payload);
    });
  }

  on(method, fn) {
    if (!this.eventListeners.has(method)) this.eventListeners.set(method, []);
    this.eventListeners.get(method).push(fn);
  }

  // Evaluate JS in the page; returns the value (JSON-serializable).
  async eval(expression, awaitPromise = true) {
    const r = await this.send('Runtime.evaluate', {
      expression, returnByValue: true, awaitPromise,
    });
    if (r.exceptionDetails) {
      throw new Error('page eval failed: ' + JSON.stringify(r.exceptionDetails).slice(0, 500));
    }
    return r.result ? r.result.value : undefined;
  }

  async screenshot(path) {
    const fs = require('fs');
    const r = await this.send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path, Buffer.from(r.data, 'base64'));
    return path;
  }

  close() { try { this.sock && this.sock.destroy(); } catch {} }
}

async function connectPage(port = 9222, urlHint = '') {
  const wsUrl = await getPageWsUrl(port, urlHint);
  const cdp = new Cdp();
  await cdp.connect(wsUrl);
  return cdp;
}

module.exports = { connectPage, getTargets };

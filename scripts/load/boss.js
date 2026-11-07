// k6 сценарий «босс»: игроки заходят в босс-арену (POPGO_START_LEVEL=3).
// Запуск: k6 run -e HOST=ws://127.0.0.1:8080 -e VUS=30 -e DUR=60s scripts/load/boss.js
import ws from "k6/ws";
import sleep from "k6";

const HOST = __ENV.HOST || "ws://127.0.0.1:8080";

export const options = { vus: Number(__ENV.VUS || 30), duration: __ENV.DUR || "60s" };

function u32(v) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, v, true); return b; }
function u16(v) { const b = new Uint8Array(2); new DataView(b.buffer).setUint16(0, v, true); return b; }
function str(s) { const e = new TextEncoder().encode(s); const l = new Uint8Array(8); new DataView(l.buffer).setBigUint64(0, BigInt(e.length), true); return new Uint8Array([...l, ...e]); }
function f32(v) { const b = new Uint8Array(4); new DataView(b.buffer).setFloat32(0, v, true); return b; }
const hello = (n) => new Uint8Array([...u32(0), ...u16(6), ...str(n), new Uint8Array([0])]);
const create = (n) => new Uint8Array([...u32(2), ...str(n), new Uint8Array([0]), ...u32(2), new Uint8Array([10, 1, 1]), ...f32(1), new Uint8Array([0])]);
const ready = () => Buffer.concat([u32(5), Buffer.from([1])]);
const start = () => u32(6);
const paddle = (x) => new Uint8Array([...u32(9), ...f32(x)]);
const action = () => u32(10);

export default function () {
  const name = "boss-" + __VU;
  ws.connect(HOST + "/ws", {}, (socket) => {
    socket.on("open", () => socket.send(hello(name)));
    socket.on("message", (data) => {
      // тег 3 = RoomState: хост готовится и стартует
      const tag = new DataView(data.buffer || data).getUint32(0, true) & 0xFF;
      if (tag === 4) { socket.send(ready()); socket.setTimeout(() => socket.send(start()), 500); }
      if (tag === 6) socket.send(action()); // снапшот — запускаем мяч
    });
    socket.setInterval(() => socket.send(paddle(100 + Math.random() * 600)), 100);
    sleep(10);
  });
}

// k6 сценарий «дым»: N игроков заходят, создают комнаты, играют.
// Запуск: k6 run -e HOST=ws://127.0.0.1:8080 -e VUS=50 -e DUR=60s scripts/load/smoke.js
import ws from "k6/ws";
import { check, sleep } from "k6";

const HOST = __ENV.HOST || "ws://127.0.0.1:8080";
const VUS = Number(__ENV.VUS || 50);
const DUR = __ENV.DUR || "60s";

export const options = {
  vus: VUS,
  duration: DUR,
  thresholds: {
    ws_connecting: ["p(95)<500"],
    "ws_msgs_sent": ["count>0"],
  },
};

function u32(v) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, v, true); return b; }
function u16(v) { const b = new Uint8Array(2); new DataView(b.buffer).setUint16(0, v, true); return b; }
function str(s) { const e = new TextEncoder().encode(s); const l = new Uint8Array(8); new DataView(l.buffer).setBigUint64(0, BigInt(e.length), true); return new Uint8Array([...l, ...e]); }
function hello(name) { return new Uint8Array([...u32(0), ...u16(6), ...str(name), new Uint8Array([0])]); }
function roomList() { return u32(1); }
function paddle(x) { return new Uint8Array([...u32(9), ...(() => { const b = new Uint8Array(4); new DataView(b.buffer).setFloat32(0, x, true); return b; })()]); }

export default function () {
  const name = "k6-" + __VU + "-" + __ITER;
  ws.connect(HOST + "/ws", {}, (socket) => {
    socket.on("open", () => socket.send(hello(name)));
    socket.on("message", () => {});
    socket.setInterval(() => socket.send(roomList()), 1000);
    socket.setInterval(() => socket.send(paddle(Math.random() * 800)), 100);
    sleep(2);
  });
}

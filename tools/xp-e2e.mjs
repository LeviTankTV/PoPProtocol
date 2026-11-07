// E2E XP: одиночный матч в режиме «Волна» с автопилотом до MatchOver,
// затем проверка /api/leaderboard — XP должен начислиться.
// Запуск: node tools/xp-e2e.mjs [port]
"use strict";

import crypto from "node:crypto";

function u32(v) { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b; }
function u16(v) { const b = Buffer.alloc(2); b.writeUInt16LE(v); return b; }
function u64(v) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b; }
function f32(v) { const b = Buffer.alloc(4); b.writeFloatLE(v); return b; }
function str(s) { const u = Buffer.from(s, "utf8"); return Buffer.concat([u64(u.length), u]); }
function opt(p) { return p === null ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), p]); }

class R {
  constructor(buf) { this.b = buf; this.i = 0; }
  u8() { return this.b[this.i++]; }
  u16() { const v = this.b.readUInt16LE(this.i); this.i += 2; return v; }
  u32() { const v = this.b.readUInt32LE(this.i); this.i += 4; return v; }
  u64() { const v = this.b.readBigUInt64LE(this.i); this.i += 8; return Number(v); }
  f32() { const v = this.b.readFloatLE(this.i); this.i += 4; return v; }
  str() { const n = this.u64(); const s = this.b.subarray(this.i, this.i + n).toString("utf8"); this.i += n; return s; }
  opt(fn) { const t = this.u8(); return t ? fn() : null; }
  vec(fn) { const n = this.u64(); const out = []; for (let k = 0; k < n; k++) out.push(fn()); return out; }
}

function decode(buf) {
  const r = new R(buf);
  const tag = r.u32();
  switch (tag) {
    case 0: return { t: "Welcome", player_id: r.u32(), token: r.str(), name: r.str() };
    case 1: return { t: "Error", code: r.u32(), text: r.str() };
    case 3: { const n = r.u64(); const first = { code: "" }; if (n > 0) { r.u32(); first.code = r.str(); } return { t: "RoomList", n, code: first.code }; }
    case 4: { const room_id = r.u32(); r.str(); r.str(); r.u32(); r.u8(); r.u8(); r.u8(); r.f32(); r.u32(); r.u32();
      const readPI = () => ({ id: r.u32(), name: r.str(), ready: !!r.u8(), is_bot: !!r.u8(), bot: r.opt(() => r.u32()), score: r.u32(), lives: r.u8(), color: r.u8(), xp: r.u32(), online: !!r.u8() });
      const players = r.vec(readPI);
      return { t: "RoomState", room_id, players, phase: r.i >= 0 ? 0 : 0 };
    }
    case 6: {
      const tick = r.u32(); r.u16(); r.u16();
      const phase = r.u32(); const level = r.u32();
      const nP = r.u64(); const players = [];
      for (let k = 0; k < nP; k++) players.push({ id: r.u32(), x: r.u16(), w: r.u16(), state: r.u32() });
      const nB = r.u64(); const balls = [];
      for (let k = 0; k < nB; k++) balls.push({ id: r.u16(), owner: r.u32(), x: r.u16(), y: r.u16() });
      const nU = r.u64(); for (let k = 0; k < nU; k++) { r.u16(); r.u8(); r.u8(); r.u8(); }
      return { t: "Snapshot", tick, phase, level, players, balls };
    }
    case 8: return { t: "MatchOver", reason: r.u32() };
    default: return { t: "Other", tag };
  }
}

const port = process.argv[2] || "18408";
const NAME = "XPTest" + (crypto.randomBytes(2).readUInt16LE(0));
const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
let myId = 0, code = "", over = null, launched = false, lastPaddle = -1;

const t0 = Date.now();
const watchdog = setTimeout(() => { console.log("XP-E2E TIMEOUT"); process.exit(1); }, 240000);

ws.onopen = () => ws.send(Buffer.concat([u32(0), u16(6), str(NAME), opt(null)]));
ws.onmessage = async (ev) => {
  const d = ev.data;
  const buf = Buffer.isBuffer(d) ? d : (d instanceof Blob ? Buffer.from(await d.arrayBuffer()) : Buffer.from(d.buffer ? d.buffer : d));
  const m = decode(buf);
  if (m.t === "Welcome") {
    myId = m.player_id;
    ws.send(Buffer.concat([u32(2), str("XP-комната"), opt(null), u32(0), Buffer.from([10, 1, 1]), f32(1.0)])); // mode 0 = Coop
  }
  if (m.t === "RoomState" && !code) {
    // запрашиваем список один раз, чтобы получить код
    code = "?";
    ws.send(u32(1));
  }
  if (m.t === "RoomList" && m.code && code === "?") {
    code = m.code;
    ws.send(Buffer.concat([u32(5), Buffer.from([1])])); // ready
    ws.send(u32(6)); // start
  }
  if (m.t === "Snapshot") {
    // AFK-путь: только запуск мячей (забитый в Ready), ракетку ведёт АФК-бот
    const readyBall = m.balls.find((b) => b.y >= 520 && b.owner === myId);
    if (readyBall) ws.send(u32(9));
  }
  if (m.t === "MatchOver") {
    over = m;
    clearTimeout(watchdog);
    ws.close();
    finish();
  }
};

async function finish() {
  const top = await fetch(`http://127.0.0.1:${port}/api/leaderboard`).then((r) => r.json());
  const me = top.find((e) => e.name === NAME);
  const secs = ((Date.now() - t0) / 1000).toFixed(0);
  console.log(`матч завершён за ${secs} с, причина=${over.reason}`);
  if (me && me.xp > 0) console.log(`XP-E2E OK: ${me.name} XP=${me.xp}`);
  else console.log(`XP-E2E FAIL: ${JSON.stringify(me)}`);
  process.exit(me && me.xp > 0 ? 0 : 1);
}
ws.onerror = (e) => { console.log("WS ERR", e.message); process.exit(1); };

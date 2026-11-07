// PoP - Protocol: бинарный кодек клиента (bincode, little-endian, fixed-width).
// Зеркало crates/core/src/protocol.rs — варианты enum только в конец, не переупорядочивать.
"use strict";

class W {
  constructor() { this.parts = []; }
  u8(v) { this.parts.push(Uint8Array.of(v & 0xFF)); return this; }
  u16(v) { const b = new ArrayBuffer(2); new DataView(b).setUint16(0, v, true); this.parts.push(new Uint8Array(b)); return this; }
  u32(v) { const b = new ArrayBuffer(4); new DataView(b).setUint32(0, v >>> 0, true); this.parts.push(new Uint8Array(b)); return this; }
  u64(v) { const b = new ArrayBuffer(8); new DataView(b).setBigUint64(0, BigInt(v), true); this.parts.push(new Uint8Array(b)); return this; }
  f32(v) { const b = new ArrayBuffer(4); new DataView(b).setFloat32(0, v, true); this.parts.push(new Uint8Array(b)); return this; }
  str(s) { const u = new TextEncoder().encode(s); this.u64(u.length); this.parts.push(u); return this; }
  opt(has, fn) { if (has) { this.u8(1); fn(); } else this.u8(0); return this; }
  bytes() { const total = this.parts.reduce((n, p) => n + p.length, 0); const out = new Uint8Array(total); let i = 0; for (const p of this.parts) { out.set(p, i); i += p.length; } return out; }
}

class R {
  constructor(buf) { this.dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength); this.i = 0; }
  u8() { return this.dv.getUint8(this.i++); }
  u16() { const v = this.dv.getUint16(this.i, true); this.i += 2; return v; }
  u32() { const v = this.dv.getUint32(this.i, true); this.i += 4; return v; }
  u64() { const v = this.dv.getBigUint64(this.i, true); this.i += 8; return Number(v); }
  f32() { const v = this.dv.getFloat32(this.i, true); this.i += 4; return v; }
  str() { const n = this.u64(); const s = new TextDecoder().decode(new Uint8Array(this.dv.buffer, this.dv.byteOffset + this.i, n)); this.i += n; return s; }
  opt(fn) { const t = this.u8(); return t ? fn() : null; }
  vec(fn) { const n = this.u64(); const out = []; for (let k = 0; k < n; k++) out.push(fn()); return out; }
}

const PROTOCOL_VERSION = 6;

// Явные функции кодирования: читаемость важнее краткости.
function mHello(name, token) {
  const w = new W().u32(0).u16(PROTOCOL_VERSION).str(name);
  if (token != null) w.u8(1).str(token); else w.u8(0);
  return w.bytes();
}
const mRoomList = () => new W().u32(1).bytes();
function mCreateRoom(name, password, mode, settings) {
  const w = new W().u32(2).str(name);
  if (password) w.u8(1).str(password); else w.u8(0);
  w.u32(mode);
  w.u8(settings.max_players).u8(settings.events_enabled ? 1 : 0).u8(settings.bosses_enabled ? 1 : 0).f32(settings.speed_factor);
  return w.bytes();
}
function mJoinRoom(code, password) {
  const w = new W().u32(3).str(code);
  if (password) w.u8(1).str(password); else w.u8(0);
  return w.bytes();
}
const mLeave = () => new W().u32(4).bytes();
const mReady = (b) => new W().u32(5).u8(b ? 1 : 0).bytes();
const mStart = () => new W().u32(6).bytes();
const mKick = (id) => new W().u32(7).u32(id).bytes();
const mPaddleX = (x) => new W().u32(8).f32(x).bytes();
const mAction = () => new W().u32(9).bytes();
const mChat = (t) => new W().u32(10).str(t).bytes();
const mEmote = (n) => new W().u32(11).u8(n).bytes();
const mPing = (n) => new W().u32(12).u64(n).bytes();
const mAddBot = (level) => new W().u32(13).u32(level).bytes();
const mRemoveBot = (id) => new W().u32(14).u32(id).bytes();
const mSetColor = (color) => new W().u32(15).u8(color).bytes();
const mSetUlt = (ult) => new W().u32(16).u8(ult).bytes();
const mUseUlt = () => new W().u32(17).bytes();

function decode(buf) {
  const r = new R(buf);
  switch (r.u32()) {
    case 0: return { t: "Welcome", player_id: r.u32(), token: r.str(), name: r.str() };
    case 1: return { t: "Error", code: r.u32(), text: r.str() };
    case 2: return { t: "Pong", n: r.u64() };
    case 3: return { t: "RoomList", list: r.vec(() => ({ room_id: r.u32(), code: r.str(), name: r.str(), mode: r.u32(), players: r.u8(), max_players: r.u8(), has_password: !!r.u8(), in_progress: !!r.u8() })) };
    case 4: {
      const st = readRoomState(r);
      return { t: "RoomState", ...st };
    }
    case 5: return { t: "MatchStarted" };
    case 6: return readSnapshot(r);
    case 7: return { t: "LevelCleared", level: r.u32() };
    case 8: return { t: "MatchOver", reason: r.u32(), scores: r.vec(() => ({ player_id: r.u32(), name: r.str(), score: r.u32(), place: r.u32(), max_combo: r.u32(), bricks: r.u32(), boss_hits: r.u32() })) };
    case 9: return { t: "EventStarted", event: r.u32(), duration_secs: r.f32() };
    case 10: return { t: "EventEnded", event: r.u32() };
    case 11: return { t: "Chat", player_id: r.u32(), text: r.str() };
    case 12: return { t: "Emote", player_id: r.u32(), emote: r.u8() };
    case 13: return { t: "PlayerJoined", ...readPlayerInfo(r) };
    case 14: return { t: "PlayerLeft", player_id: r.u32() };
    case 15: return { t: "HostChanged", player_id: r.u32() };
    default: return { t: "Unknown" };
  }
}

function readPlayerInfo(r) {
  return {
    id: r.u32(), name: r.str(), ready: !!r.u8(), is_bot: !!r.u8(),
    bot_level: r.opt(() => r.u32()), score: r.u32(), lives: r.u8(), color: r.u8(), xp: r.u32(),
    online: !!r.u8(), ult: r.u8(),
  };
}

function readRoomState(r) {
  const room_id = r.u32(), code = r.str(), name = r.str(), mode = r.u32();
  const settings = { max_players: r.u8(), events_enabled: !!r.u8(), bosses_enabled: !!r.u8(), speed_factor: r.f32() };
  const host_id = r.u32(), phase = r.u32();
  const players = r.vec(() => readPlayerInfo(r));
  const spectators = r.vec(() => readPlayerInfo(r));
  return { room_id, code, name, mode, settings, host_id, phase, players, spectators };
}

function readSnapshot(r) {
  const tick = r.u32(), field_w = r.u16() / 10, field_h = r.u16() / 10;
  const phase = r.u32(), level = r.u32();
  const players = r.vec(() => ({ id: r.u32(), x: r.u16() / 10, w: r.u16() / 10, state: r.u32(), ult_charge: r.u8() }));
  const balls = r.vec(() => ({ id: r.u16(), owner_id: r.u32(), x: r.u16() / 10, y: r.u16() / 10 }));
  const bricks_upsert = r.vec(() => ({ id: r.u16(), c: r.u8(), r: r.u8(), hp: r.u8(), kind: r.u8() }));
  const bricks_removed = r.vec(() => r.u16());
  const capsules = r.vec(() => ({ id: r.u16(), x: r.u16() / 10, y: r.u16() / 10, kind: r.u8() }));
  const bullets = r.vec(() => ({ id: r.u16(), owner_id: r.u32(), x: r.u16() / 10, y: r.u16() / 10 }));
  r.vec(() => { r.u16(); r.u16(); r.str(); r.u8(); });             // floats
  const boss = r.opt(() => ({ kind: r.u8(), hp: r.u32(), max_hp: r.u32(), phase: r.u8(), x: r.u16() / 10, y: r.u16() / 10, shields: r.vec(() => ({ hp: r.u8(), angle: r.u8() })) }));
  const events = r.vec(() => ({ event: r.u32(), remaining_secs: r.f32() }));
  const combo = r.vec(() => ({ player_id: r.u32(), combo: r.u8() }));
  const scores = r.vec(() => ({ player_id: r.u32(), score: r.u32(), lives: r.u8() }));
  const fx = r.u8();
  const projs = r.vec(() => ({ id: r.u16(), x: r.u16() / 10, y: r.u16() / 10, kind: r.u8() }));
  return { t: "Snapshot", tick, field_w, field_h, phase, level, players, balls, bricks_upsert, bricks_removed, capsules, bullets, events, boss, combo, scores, fx, projs };
}

window.PopProto = { PROTOCOL_VERSION, mHello, mRoomList, mCreateRoom, mJoinRoom, mLeave, mReady, mStart, mKick, mPaddleX, mAction, mChat, mEmote, mPing, mAddBot, mRemoveBot, mSetColor, mSetUlt, mUseUlt, decode };

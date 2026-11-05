// E2E smoke: два игрока по WS — Hello/Create/Join/Ready/StartMatch.
// Запуск: node tools/ws-smoke.mjs [port]
"use strict";

/* ---------- bincode (little-endian, fixed-width) ---------- */
function u32(v) { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b; }
function u16(v) { const b = Buffer.alloc(2); b.writeUInt16LE(v); return b; }
function u64(v) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b; }
function f32(v) { const b = Buffer.alloc(4); b.writeFloatLE(v); return b; }
function str(s) { const u = Buffer.from(s, "utf8"); return Buffer.concat([u64(u.length), u]); }
function opt(p) { return p === null ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), p]); }

const enc = {
  hello: (name, token) => Buffer.concat([u32(0), u16(4), str(name), opt(token ? str(token) : null)]),
  addBot: (level) => Buffer.concat([u32(13), u32(level)]),
  removeBot: (id) => Buffer.concat([u32(14), u32(id)]),
  createRoom: (name, pass) => Buffer.concat([u32(2), str(name), opt(pass ? str(pass) : null), u32(0), Buffer.from([10, 1, 1]), f32(1.0)]),
  joinRoom: (code, pass) => Buffer.concat([u32(3), str(code), opt(pass ? str(pass) : null)]),
  paddleX: (x) => Buffer.concat([u32(8), f32(x)]),
  action: () => u32(9),
  setReady: (b) => Buffer.concat([u32(5), Buffer.from([b ? 1 : 0])]),
  startMatch: () => u32(6),
};

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

// PlayerInfo: общий читатель (v2: онлайн-флаг в конце)
function readPI(r) {
  return { id: r.u32(), name: r.str(), ready: !!r.u8(), is_bot: !!r.u8(), bot: r.opt(() => r.u32()), score: r.u32(), lives: r.u8(), color: r.u8(), xp: r.u32(), online: !!r.u8() };
}
function readRoomState(r) {
  const room_id = r.u32(), code = r.str(), name = r.str(), mode = r.u32();
  r.u8(); r.u8(); r.u8(); r.f32(); // settings
  const host_id = r.u32(), phase = r.u32();
  const players = r.vec(() => readPI(r));
  const spectators = r.vec(() => readPI(r));
  return { room_id, code, name, mode, host_id, phase, players, spectators };
}

// ServerMsg: 0 Welcome, 1 Error, 2 Pong, 3 RoomList, 4 RoomState, 5 MatchStarted,
// 6 Snapshot, ..., 13 PlayerJoined, 14 PlayerLeft, 15 HostChanged
function decode(buf) {
  const r = new R(buf);
  const tag = r.u32();
  switch (tag) {
    case 0: return { t: "Welcome", player_id: r.u32(), token: r.str(), name: r.str() };
    case 1: return { t: "Error", code: r.u32(), text: r.str() };
    case 3: {
      const n = r.u64(); const list = [];
      for (let k = 0; k < n; k++) list.push({ room_id: r.u32(), code: r.str(), name: r.str(), mode: r.u32(), players: r.u8(), max: r.u8(), has_pass: r.u8(), in_progress: r.u8() });
      return { t: "RoomList", list };
    }
    case 4: {
      const room_id = r.u32(); const code = r.str(); const name = r.str(); const mode = r.u32();
      r.u8(); r.u8(); r.u8(); r.f32(); // settings
      const host_id = r.u32(); const phase = r.u32();
      const players = r.u64(); const pl = [];
      for (let k = 0; k < players; k++) pl.push({ id: r.u32(), name: r.str(), ready: r.u8(), is_bot: r.u8(), bot: r.opt(() => r.u32()), score: r.u32(), lives: r.u8(), color: r.u8(), xp: r.u32(), online: r.u8() });
      const specs = r.u64();
      const sp = [];
      for (let k = 0; k < specs; k++) sp.push({ id: r.u32(), name: r.str(), ready: r.u8(), is_bot: r.u8(), bot: r.opt(() => r.u32()), score: r.u32(), lives: r.u8(), color: r.u8(), xp: r.u32(), online: r.u8() });
      return { t: "RoomState", room_id, code, name, mode, host_id, phase, players: pl, spectators: sp };
    }
    case 5: return { t: "MatchStarted" };
    case 6: {
      const tick = r.u32(); const fw = r.u16(); const fh = r.u16();
      const phase = r.u32(); const level = r.u32();
      const nP = r.u64(); const players = [];
      for (let k = 0; k < nP; k++) players.push({ id: r.u32(), x: r.u16(), w: r.u16(), state: r.u32() });
      const nB = r.u64(); const balls = [];
      for (let k = 0; k < nB; k++) balls.push({ id: r.u16(), owner: r.u32(), x: r.u16(), y: r.u16() });
      const nU = r.u64(); for (let k = 0; k < nU; k++) { r.u16(); r.u8(); r.u8(); r.u8(); }
      const nR = r.u64(); for (let k = 0; k < nR; k++) r.u16();
      const nC = r.u64(); for (let k = 0; k < nC; k++) { r.u16(); r.u16(); r.u16(); r.u8(); }
      const nBl = r.u64(); for (let k = 0; k < nBl; k++) { r.u16(); r.u32(); r.u16(); r.u16(); }
      const nF = r.u64(); for (let k = 0; k < nF; k++) { r.u16(); r.u16(); r.str(); r.u8(); }
      if (r.u8() === 1) { r.u8(); r.u32(); r.u32(); r.u8(); r.u16(); r.u16(); }
      const nE = r.u64(); for (let k = 0; k < nE; k++) { r.u32(); r.f32(); }
      const nCm = r.u64(); for (let k = 0; k < nCm; k++) { r.u32(); r.u8(); }
      return { t: "Snapshot", tick, phase, level, players, balls, bricks: nU };
    }
    case 13: return { t: "PlayerJoined", ...readPI(r) }; // Box<PlayerInfo> — без тега Option
    case 14: return { t: "PlayerLeft", player_id: r.u32() };
    case 15: return { t: "HostChanged", player_id: r.u32() };
    default: return { t: "Other", tag };
  }
}

/* ---------- тест ---------- */
const port = process.argv[2] || "18082";
const url = `ws://127.0.0.1:${port}/ws`;

function connect(name) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    const inbox = [];
    const waiters = [];
    ws.onmessage = (e) => {
      const raw = Buffer.from(new Uint8Array(e.data));
      let msg;
      try { msg = decode(raw); } catch (err) {
        console.error("DECODE CRASH tag=" + raw.readUInt32LE(0) + " len=" + raw.length);
        console.error(raw.toString("hex").slice(0, 300));
        throw err;
      }
      inbox.push(msg);
      const wi = waiters.findIndex((w) => w.pred(msg));
      if (wi >= 0) waiters.splice(wi, 1)[0].resolve(msg);
    };
    ws.onerror = (e) => reject(new Error(name + " ws error"));
    ws.onopen = () => resolve({
      ws,
      send: (buf) => ws.send(buf),
      wait: (pred, label, timeoutMs = 3000) => {
        const hit = inbox.find(pred);
        if (hit) return Promise.resolve(hit);
        return new Promise((res, rej) => {
          waiters.push({ pred, resolve: res });
          setTimeout(() => rej(new Error(name + ": timeout — " + label)), timeoutMs);
        });
      },
    });
  });
}

const is = (t) => (m) => m.t === t;

async function main() {
  // Ниже ники уникальны за прогон: MongoDB помнит игроков между запусками.
  const tag = String(Date.now() % 100000);
  // Игрок 1: вход и создание комнаты
  const p1 = await connect("P1");
  p1.send(enc.hello("Тима-" + tag, null));
  const w1 = await p1.wait(is("Welcome"), "Welcome P1");
  console.log("P1 Welcome: id=" + w1.player_id + " token=" + w1.token.slice(0, 8) + "…");
  p1.send(enc.createRoom("Комната теста", null));
  const st1 = await p1.wait((m) => m.t === "RoomState" && m.host_id === w1.player_id, "RoomState P1");
  console.log("Комната создана: код=" + st1.code + " игроков=" + st1.players.length);

  // Игрок 2: вход и присоединение по коду
  const p2 = await connect("P2");
  p2.send(enc.hello("Боб-" + tag, null));
  const w2 = await p2.wait(is("Welcome"), "Welcome P2");
  p2.send(enc.joinRoom(st1.code, null));
  const pj = await p2.wait(is("RoomState"), "RoomState P2");
  console.log("P2 вошёл, игроков=" + pj.players.length + ", фаза=" + pj.phase);
  await p1.wait((m) => m.t === "PlayerJoined", "P1 видит PlayerJoined");
  console.log("P1 увидел PlayerJoined");

  // Шаг 11: хост добавляет бота (нормал) — становится трое
  p1.send(enc.addBot(1));
  const withBot = await p1.wait((m) => m.t === "RoomState" && m.players.length === 3, "RoomState с ботом");
  const bot = withBot.players.find((p) => p.is_bot);
  console.log("Бот в комнате: id=" + bot.id + " имя=" + bot.name);
  if (!bot.ready) throw new Error("бот должен быть готов сразу");

  // Неверный код — ошибка
  const p3 = await connect("P3");
  p3.send(enc.hello("В-" + tag, null));
  await p3.wait(is("Welcome"), "Welcome P3");
  p3.send(enc.joinRoom("000000", null));
  const err = await p3.wait(is("Error"), "ошибка неверного кода");
  console.log("Неверный код отклонён: code=" + err.code);

  // Ready-check: старт без готовности запрещён
  p1.send(enc.startMatch());
  await p1.wait(is("Error"), "старт без готовности");
  console.log("Старт без готовности отклонён");
  p2.send(enc.setReady(true));
  p1.send(enc.setReady(true));
  await p1.wait((m) => m.t === "RoomState" && m.players.every((p) => p.ready), "готовность");
  p1.send(enc.startMatch());
  const ms = await p1.wait(is("MatchStarted"), "MatchStarted");
  console.log("Матч начат:", ms.t);

  // Шаг 10: реконнект — P2 обрывает соединение и возвращается по токену
  p2.ws.close();
  await p1.wait((m) => m.t === "RoomState" && m.players.some((p) => p.id === w2.player_id && !p.online), "P2 офлайн");
  console.log("P2 отвалился, слот держится (бот ведёт ракетку)");
  await new Promise((r) => setTimeout(r, 1200));
  const p2b = await connect("P2b");
  p2b.send(enc.hello("Боб-" + tag, w2.token));
  const w2b = await p2b.wait(is("Welcome"), "Welcome P2b");
  if (w2b.player_id !== w2.player_id) throw new Error("токен не восстановил игрока");
  p2b.send(enc.joinRoom(st1.code, null));
  const back = await p2b.wait((m) => m.t === "RoomState" && m.players.some((p) => p.id === w2.player_id && p.online), "P2 снова онлайн");
  console.log("P2 вернулся: игроков=" + back.players.length + " · матч продолжается, тик=" +
    (await p2b.wait(is("Snapshot"), "снапшот после реконнекта")).tick);

  // REST-список: наша комната должна быть в нём с верными полями
  const rest = await fetch(`http://127.0.0.1:${port}/api/rooms`).then((r) => r.json());
  console.log("REST /api/rooms: комнат=" + rest.length);
  const mine = Array.isArray(rest) && rest.find((r) => r.code === st1.code);
  if (!mine || mine.in_progress !== true || mine.players !== 3) {
    throw new Error("REST-список не совпал с ожиданием (2 игрока + бот)");
  }

  // Матч: снапшоты 30 Гц, запуск мяча, движение ракетки
  const snap0 = await p1.wait(is("Snapshot"), "первый снапшот");
  console.log(`Снапшот: тик=${snap0.tick} фаза=${snap0.phase} кирпичей=${snap0.bricks} мячей=${snap0.balls.length}`);
  if (snap0.bricks === 0 || snap0.balls.length !== 3) throw new Error("начальный снапшот неверный (3 мяча: 2 игрока + бот)");
  p1.send(enc.action());
  p2.send(enc.action());
  const flying = await p1.wait((m) => m.t === "Snapshot" && m.phase === 1 && m.balls.some((b) => b.y < 5000), "мяч полетел вверх");
  console.log("Мяч в игре: y=" + flying.balls[0].y / 10);
  p1.send(enc.paddleX(700));
  p1.send(enc.paddleX(700));
  await p1.wait((m) => m.t === "Snapshot" && m.players.some((p) => p.id === w1.player_id && p.x > 6000), "ракетка двинулась к 700");
  console.log("Ракетка отреагировала на PaddleX");

  p1.ws.close(); p2b.ws.close(); p3.ws.close();
  console.log("WS-SMOKE OK");
  process.exit(0);
}

main().catch((e) => { console.error("WS-SMOKE FAIL:", e.message); process.exit(1); });

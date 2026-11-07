// PoP - Protocol: игровой клиент (PixiJS).
// Экраны: меню -> комната -> матч. Рендер снапшотов с интерполяцией ~100 мс.
"use strict";

const P = window.PopProto;

/* ---------- i18n ---------- */
const I18N = {
  ru: {
    connecting: "подключение…", online: "сервер онлайн", offline: "сервер недоступен, переподключение…",
    roomName: "название комнаты", roomPass: "пароль (необязательно)", create: "Создать",
    joinCode: "код комнаты", joinPass: "пароль", joinByCode: "Войти по коду", refresh: "↻ обновить",
    leaderboard: "🏆 Топ игроков", ready: "Готов", notReady: "Не готов", secCreate: "🏠 Создать комнату", secJoin: "🚪 Войти по коду", secRooms: "📋 Открытые комнаты", secTop: "🏆 Зал славы", joinHint: "код скажет друг, который создал комнату", start: "▶ Старт", leave: "Выйти", exitMenu: "Выйти в меню", becomePlayer: "Стать игроком", ultPick: "Ульта:", ultNone: "Нет", ultIce: "❄ Ледяная волна", ultMeteor: "☄ Метеорит", ultMagnet: "🧲 Магнит", ultReady: "заряд",
    bots: "Боты:", botLazy: "+ Лентяй", botNormal: "+ Обычный", botPro: "+ Профи",
    hint: "мышь — ракетка · клик/пробел — запуск · 1–8 — эмоции",
    noRooms: "Пока нет комнат — создайте первую!", enter: "Войти", inProgress: "▶ идёт матч", lobbySt: "лобби",
    you: "(вы)", spectator: "зритель", readyMark: "✅ готов", kick: "кик", remove: "убрать",
    level: "ур.", chatPh: "сообщение…", victory: "🏆 ПОБЕДА!", gameOver: "ИГРА ОКОНЧЕНА",
    backToLobby: "Комната вернулась в лобби…", nameTaken: "ник занят, выберите другой",
    nameQuota: "ник: 1–16 символов",
    winTitle: "🏆 ПОБЕДА!", loseTitle: "ИГРА ОКОНЧЕНА", exhausted: "ВОЛНА ИСЧЕРПАНА", topEmpty: "сыграйте матч — и вы здесь", refreshTop: "↻ обновить топ",
  },
  en: {
    connecting: "connecting…", online: "server online", offline: "server unreachable, reconnecting…",
    roomName: "room name", roomPass: "password (optional)", create: "Create",
    joinCode: "room code", joinPass: "password", joinByCode: "Join by code", refresh: "↻ refresh",
    leaderboard: "🏆 Leaderboard", ready: "Ready", notReady: "Not ready", secCreate: "🏠 Create a room", secJoin: "🚪 Join by code", secRooms: "📋 Open rooms", secTop: "🏆 Hall of fame", joinHint: "ask the friend who created the room for the code", start: "▶ Start", leave: "Leave", exitMenu: "Exit to menu", becomePlayer: "Become a player", ultPick: "Ultimate:", ultNone: "None", ultIce: "❄ Ice Wave", ultMeteor: "☄ Meteor", ultMagnet: "🧲 Magnet", ultReady: "charges",
    bots: "Bots:", botLazy: "+ Lazy", botNormal: "+ Normal", botPro: "+ Pro",
    hint: "mouse — paddle · click/space — launch · 1–8 — emotes",
    noRooms: "No rooms yet — create the first one!", enter: "Join", inProgress: "▶ playing", lobbySt: "lobby",
    you: "(you)", spectator: "spectator", readyMark: "✅ ready", kick: "kick", remove: "remove",
    level: "lvl", chatPh: "message…", victory: "🏆 VICTORY!", gameOver: "GAME OVER",
    backToLobby: "Room is back to lobby…", nameTaken: "nickname taken, pick another",
    nameQuota: "nickname: 1–16 chars",
    winTitle: "🏆 VICTORY!", loseTitle: "GAME OVER", exhausted: "WAVE EXHAUSTED", topEmpty: "play a match — you will be here", refreshTop: "↻ refresh top",
  },
};
let leaderboardShown = false;
let LANG = localStorage.getItem("pop_lang") || (navigator.language || "ru").slice(0, 2);
if (!I18N[LANG]) LANG = "ru";
const t = (k) => I18N[LANG][k] ?? I18N.ru[k] ?? k;

// цвет из палитры с числовым guard'ом (защита от undefined в переходные кадры)
function pcolor(v) { return COLORS[Number.isFinite(v) ? v % COLORS.length : 5]; }

/* ---------- экран загрузки / переподключения ---------- */
function showLoading(reconnecting) {
  const el = document.getElementById("loading-screen");
  if (!el) return;
  const txt = document.getElementById("loading-text");
  if (txt) txt.innerHTML = (reconnecting ? t("reconnecting") : t("connecting")) + '<span class="dots"><span>.</span><span>.</span><span>.</span></span>';
  el.classList.remove("hidden");
}
function hideLoading() {
  const el = document.getElementById("loading-screen");
  if (el) el.classList.add("hidden");
}

function applyLang() {
  document.querySelectorAll("[data-i18n]").forEach((el) => { el.textContent = t(el.dataset.i18n); });
  document.querySelectorAll("[data-i18n-ph]").forEach((el) => { el.placeholder = t(el.dataset.i18nPh); });
  document.getElementById("btn-lang").textContent = LANG === "ru" ? "EN" : "RU";
  setStatus(S.connected ? t("online") : t("connecting"), S.connected ? "ok" : "");
  const lb = document.getElementById("btn-leaderboard");
  if (lb) lb.textContent = t("leaderboard");
}
const EVENT_NAMES = ["МЕТЕОРИТНЫЙ ДОЖДЬ", "ТЬМА", "ИНВЕРСИЯ", "ЗОЛОТАЯ ЖИЛА ×3", "ЯРОСТЬ СКОРОСТИ", "КАМНЕПАД", "ФОНТАН БОНУСОВ", "ДВОЙНОЙ МЯЧ", "ЩИТ ЗЕМЛИ"];
const COLORS = ["#ff6b6b", "#feca57", "#48dbfb", "#1dd1a1", "#a55eea", "#ff9ff3", "#ee7724", "#4a69bd", "#10ac84", "#e0181d"];
const BRICK_COLORS = { 1: [0xff8a8a, 0xff5252], 2: [0xffd97a, 0xffb03a], 3: [0x7ed6f5, 0x3aa7e0], 255: [0xcfd6df, 0x9aa5b1] };
const CELL_H = 26, GRID_TOP = 58, PADDLE_H = 16, BALL_R = 11;
const INTERP_DELAY = 100;

/* ---------- состояние ---------- */
const S = {
  ws: null, connected: false,
  myId: 0, myName: "", token: localStorage.getItem("pop_token") || null,
  room: null,            // RoomState
  isHost: false,
  snapBuf: [],           // [{recv, snap}] для интерполяции
  latest: null,          // последний снапшот
  bricks: new Map(),     // id -> {c, r, hp}
  fieldW: 800, fieldH: 600,
  level: 0,
  inputX: 400, lastSend: 0,
  over: null,            // данные MatchOver
  particles: [],         // локальный сок: {x,y,vx,vy,life,t,color} + кольца {ring:true}
  particlesOn: true,
  brickMax: new Map(),   // id -> максимум hp (для трещин)
  brickBorn: new Map(),  // id -> время появления (анимация scale-in)
  trails: new Map(),     // шлейф мячей: id -> [{x,y}]
  trailT: 0,
  fpsFrames: 0, fpsAcc: 0, fps: 60,
  floatTexts: [],        // {obj,x,y,t,life} — PIXI.Text комбо-надписей
  comboSeen: new Map(),  // player_id -> последняя серия (для фейерверка)
  prevBalls: new Map(),  // id -> {y} для звука отскока
  prevSig: { bricks: 0, bullets: 0, caps: 0, bossHp: -1, lives: 0 },
  snapInt: 0, lastSnapRecv: 0,   // EMA интервала снапшотов -> адаптивная задержка
  paddleX: null,                 // предсказанная позиция СВОЕЙ ракетки (без задержки)
  overUntil: 0, overPending: false, // анимационная пауза после матча
  prevShieldHps: [],                // HP щитов с прошлого снапшота (вспышки урона)
  bgDirty: true, bricksDirty: true, bgBoss: false, // кэши статики
};

/* ---------- сеть ---------- */
function connect() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(proto + "//" + location.host + "/ws");
  ws.binaryType = "arraybuffer";
  S.ws = ws;
  ws.onopen = () => {
    S.connected = true;
    const name = localStorage.getItem("pop_name") || ("Игрок" + ((Math.random() * 900 + 100) | 0));
    ws.send(P.mHello(name, S.token));
    setStatus(t("online"), "ok");
  };
  ws.onclose = () => {
    S.connected = false;
    showLoading(true); // тематический экран переподключения
    setStatus(t("offline"), "err");
    setTimeout(connect, 1500);
  };
  ws.onmessage = (e) => handle(decodeMsg(new Uint8Array(e.data)));
}
function decodeMsg(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  // decode работает с Uint8Array-обёрткой
  return P.decode(bytes);
}
const send = (buf) => { if (S.connected) S.ws.send(buf); };

/* ---------- обработка серверных сообщений ---------- */
function handle(m) {
  switch (m.t) {
    case "Welcome":
      hideLoading();
      S.myId = m.player_id; S.myName = m.name; S.token = m.token;
      localStorage.setItem("pop_token", m.token);
      localStorage.setItem("pop_name", m.name);
      // авто-реконнект в последнюю комнату в течение окна 90 с
      const lastCode = localStorage.getItem("pop_room_code");
      if (lastCode && !S.room) {
        send(P.mJoinRoom(lastCode, localStorage.getItem("pop_room_pass")));
      } else {
        send(P.mRoomList());
      }
      break;
    case "Error": toast("⚠ " + m.text); break;
    case "RoomList": renderRoomList(m.list); break;
    case "RoomState":
      S.room = m;
      S.isHost = m.host_id === S.myId;
      localStorage.setItem("pop_room_code", m.code);
      if (m.phase === 0) {
        // после матча показываем итоги с анимацией, потом мягкий переход в лобби
        const left = (S.overUntil || 0) - performance.now();
        if (left > 60 && !S.overPending) {
          S.overPending = true;
          setTimeout(() => { S.overPending = false; S.overUntil = 0; showScreen("room"); }, left);
        } else if (!S.overPending) {
          S.overUntil = 0;
          showScreen("room");
        }
      } else {
        S.overUntil = 0;
        showScreen("game"); // реконнект/вход в идущий матч — сразу поле
      }
      break;
    case "MatchStarted":
      S.snapBuf = []; S.latest = null; S.bricks.clear(); S.over = null;
      S.paddleX = null; S.trails.clear(); S.comboSeen.clear(); S.overUntil = 0; S.overPending = false;
      if (S.ballSpawn) S.ballSpawn.clear(); if (S.prevBalls) S.prevBalls.clear();
      S.bgDirty = true; S.bricksDirty = true;
      showScreen("game");
      break;
    case "Snapshot": onSnapshot(m); break;
    case "LevelCleared": toast("Уровень " + m.level + " пройден!"); break;
    case "EventStarted": toast("⚡ " + (EVENT_NAMES[m.event] || "ЕВЕНТ") + (m.duration_secs > 0 ? " (" + Math.round(m.duration_secs) + "с)" : "")); break;
    case "EventEnded": toast("… " + (EVENT_NAMES[m.event] || "евент") + " закончился"); break;
    case "MatchOver": showOver(m); break;
    case "Chat": addChat(m.player_id, m.text); break;
    case "Emote": showEmote(m.player_id, m.emote); break;
    case "HostChanged": toast("Новый хост"); break;
    case "PlayerLeft":
      if (m.player_id === S.myId) {
        S.room = null;
        S.overUntil = 0; S.overPending = false;
        localStorage.removeItem("pop_room_code");
        showScreen("menu");
      }
      break;
  }
}

function onSnapshot(s) {
  if (s.level !== S.level) { S.bricks.clear(); S.brickMax.clear(); S.brickBorn.clear(); S.level = s.level; }
  for (const b of s.bricks_upsert) {
    if (!S.bricks.has(b.id)) S.brickBorn.set(b.id, performance.now());
    S.bricks.set(b.id, b);
    const m = S.brickMax.get(b.id) || 0;
    if (b.hp > m) S.brickMax.set(b.id, b.hp);
  }
  S.bricksDirty = true;
  for (const id of s.bricks_removed) {
    const b = S.bricks.get(id);
    if (b) {
      const cellw = s.field_w / 10;
      const col = BRICK_COLORS[S.brickMax.get(id)] || BRICK_COLORS[1] || BRICK_COLORS[0] || [0xffffff, 0xffffff];
      spawnRing(b.c * cellw + cellw / 2, GRID_TOP + b.r * CELL_H + 13, col[0], 30);
      if (S.particlesOn) spawnBurst(b.c, b.r);
      sfxBrick();
    }
    S.bricks.delete(id);
  }
  // звуковые события по дельтам снапшота (аудио-модель без серверных пушей)
  const sig = S.prevSig;
  if (s.bullets.length > sig.bullets) sfxLaser();
  if (s.capsules.length < sig.caps) sfxCapsule();
  if (s.boss && sig.bossHp >= 0 && s.boss.hp < sig.bossHp) sfxBossHit();
  const livesNow = s.scores.reduce((acc, sc) => acc + sc.lives, 0);
  if (sig.lives > 0 && livesNow < sig.lives) sfxLose();
  sig.bullets = s.bullets.length; sig.caps = s.capsules.length;
  sig.bossHp = s.boss ? s.boss.hp : -1; sig.lives = livesNow;
  // стан снарядом босса: всплывающая заморозка над ракеткой
  for (const pl of s.players) {
    const key = "stun" + pl.id;
    if (pl.state === 2 && !S[key]) {
      S[key] = true;
      if (pl.id === S.myId) toast("❄ " + (LANG === "en" ? "FROZEN — paddle stunned!" : "ЗАМОРОЗКА — ракетка скована!"));
      else floatText(pl.x, s.field_h - 80, "❄", 0x81d4fa);
    } else if (pl.state !== 2) S[key] = false;
  }
  // комбо-фейерверк: серия игрока достигла кратной 5 отметки
  for (const cm of s.combo || []) {
    const was = S.comboSeen.get(cm.player_id) || 0;
    if (cm.combo > was && cm.combo >= 5 && cm.combo % 5 === 0) {
      const ps = s.players.find((pl) => pl.id === cm.player_id);
      const info = playerInfo(cm.player_id) || {};
      const color = COLORS[(info.color || 0) % COLORS.length];
      spawnFirework(ps ? ps.x : s.field_w / 2, s.field_h - 90, color, "×" + cm.combo);
    }
    S.comboSeen.set(cm.player_id, cm.combo);
  }
  // отскок от ракетки: мяч пересёк нижнюю зону снизу вверх
  for (const b of s.balls) {
    const by = b.y / 10;
    const prev = S.prevBalls.get(b.id);
    if (prev && prev.y > s.field_h - 60 && by <= s.field_h - 60 && by > s.field_h - 130) sfxPaddle();
    S.prevBalls.set(b.id, { y: by });
  }
  if (S.prevBalls.size > s.balls.length * 3 + 10) {
    const ids = new Set(s.balls.map((b) => b.id));
    for (const id of S.prevBalls.keys()) if (!ids.has(id)) S.prevBalls.delete(id);
  }
  S.fieldW = s.field_w; S.fieldH = s.field_h;
  if (!S.latest || S.latest.field_w !== s.field_w || S.latest.field_h !== s.field_h) fitCanvas(s.field_w);
  const now = performance.now();
  if (S.lastSnapRecv) {
    S.snapInt = S.snapInt ? S.snapInt * 0.8 + (now - S.lastSnapRecv) * 0.2 : now - S.lastSnapRecv;
  }
  S.lastSnapRecv = now;
  // коррекция предсказания своей ракетки (телепорт/респаун/стан)
  const me = s.players.find((pl) => pl.id === S.myId);
  if (me) {
    const sx = me.x;
    if (S.paddleX === null || Math.abs(S.paddleX - sx) > 60 || me.state === 2) S.paddleX = sx;
  } else {
    S.paddleX = null;
  }
  S.snapBuf.push({ recv: now, snap: s });
  if (S.snapBuf.length > 10) S.snapBuf.shift();
  S.latest = s;
  updateHud(s);
}

/* ---------- экраны ---------- */
function showScreen(name) {
  for (const s of ["menu", "room", "game"]) document.getElementById("screen-" + s).style.display = s === name ? "block" : "none";
  document.getElementById("over").style.display = "none";
  if (name === "menu") { leaderboardShown = false; send(P.mRoomList()); if (window.__loadTop) window.__loadTop(); }
  if (name === "room") renderRoom();
  if (name === "game") {
    // поле всегда в зоне видимости: матч начался или реконнект — не заставляем листать
    const holder = document.getElementById("canvas-holder");
    if (holder && holder.scrollIntoView) holder.scrollIntoView({ block: "start" });
  }
}
function setStatus(text, kind) {
  const el = document.getElementById("conn");
  if (el) { el.textContent = text; el.className = kind || ""; }
}
function toast(text) {
  const box = document.getElementById("toasts");
  const el = document.createElement("div");
  el.textContent = text;
  box.appendChild(el);
  setTimeout(() => el.remove(), 2500);
}

/* меню */
function renderRoomList(list) {
  const box = document.getElementById("rooms");
  box.innerHTML = "";
  if (!list.length) { box.innerHTML = `<div class='muted'>${t("noRooms")}</div>`; return; }
  for (const r of list) {
    const row = document.createElement("div");
    row.className = "room-row";
    row.innerHTML = `<b>${escapeHtml(r.name)}</b> <span class="muted">код ${r.code}</span>
      ${r.has_password ? "🔒" : ""} ${r.in_progress ? t("inProgress") : t("lobbySt")} · ${r.players}/${r.max_players}`;
    const btn = document.createElement("button");
    btn.textContent = t("enter");
    btn.onclick = () => {
      let pass = null;
      if (r.has_password) { pass = prompt("Пароль комнаты:"); if (pass === null) return; }
      send(P.mJoinRoom(r.code, pass));
    };
    row.appendChild(btn);
    box.appendChild(row);
  }
}

/* комната */
function renderRoom() {
  const r = S.room;
  if (!r) return;
  document.getElementById("room-title").textContent = r.name + " · код " + r.code;
  // палитра: клик по цвету меняет ракетку/мячи (если свободен)
  const pal = document.getElementById("color-palette");
  if (pal) {
    pal.innerHTML = "";
    const taken = new Set([
      ...r.players.filter((p) => p.id !== S.myId).map((p) => p.color),
      ...r.spectators.filter((p) => p.id !== S.myId).map((p) => p.color),
    ]);
    const me = r.players.find((p) => p.id === S.myId) || r.spectators.find((p) => p.id === S.myId);
    document.querySelectorAll("#ult-row .ult-btn").forEach((b) => {
      b.classList.toggle("active", me && me.ult === +b.dataset.ult);
      b.disabled = !r.players.some((p) => p.id === S.myId);
    });
    for (let i = 0; i < COLORS.length; i++) {
      const dot = document.createElement("span");
      dot.className = "color-dot" + (me && me.color === i ? " chosen" : "");
      dot.style.background = COLORS[i];
      if (taken.has(i)) dot.style.opacity = "0.25";
      dot.title = taken.has(i) ? "занято" : "выбрать цвет";
      dot.onclick = () => { if (!taken.has(i)) send(P.mSetColor(i)); };
      pal.appendChild(dot);
    }
  }
  const box = document.getElementById("room-players");
  box.innerHTML = "";
  const all = [...r.players.map((p) => ({ ...p, spec: false })), ...r.spectators.map((p) => ({ ...p, spec: true }))];
  for (const p of all) {
    const row = document.createElement("div");
    row.className = "player-row";
    const color = p.online === false ? "#9aa5b1" : COLORS[p.color % COLORS.length];
    const xpLevel = Math.floor(p.xp / 100) + 1;
    row.innerHTML = `<span class="dot" style="background:${color}"></span>
      <b>${escapeHtml(p.name)}</b>${p.id === S.myId ? " " + t("you") : ""}
      <span class="muted">lvl ${xpLevel}</span>
      ${p.id === r.host_id ? " 👑" : ""}
      ${p.online === false ? " 📴" : ""}
      ${p.spec ? ` <span class='muted'>${t("spectator")}</span>` : p.ready ? ` ${t("readyMark")}` : ""}`;
    if (S.isHost && p.id !== S.myId) {
      const kick = document.createElement("button");
      kick.textContent = p.is_bot ? t("remove") : t("kick");
      kick.className = "small";
      kick.onclick = () => send(p.is_bot ? P.mRemoveBot(p.id) : P.mKick(p.id));
      row.appendChild(kick);
    }
    box.appendChild(row);
  }
  document.getElementById("btn-start").style.display = S.isHost ? "inline-block" : "none";
  // Готов нажимают все, включая хоста — сервер стартует только при полной готовности.
  // зритель при свободном слоте может стать игроком
  const joinBtn = document.getElementById("btn-join-play");
  if (joinBtn) {
    const iAmSpec = r.spectators.some((p) => p.id === S.myId);
    const freeSlot = r.players.length < r.settings.max_players && r.phase !== 2;
    joinBtn.style.display = iAmSpec && freeSlot ? "inline-block" : "none";
  }
  document.getElementById("btn-ready").style.display = "inline-block";
  const botRow = document.getElementById("bot-buttons");
  botRow.style.display = S.isHost ? "flex" : "none";
  const me = r.players.find((p) => p.id === S.myId);
  document.getElementById("btn-ready").textContent = me && me.ready ? t("notReady") : t("ready");
}

function addChat(playerId, text) {
  const log = document.getElementById("chat");
  const who = S.room ? (S.room.players.find((p) => p.id === playerId) || {}).name : null;
  const el = document.createElement("div");
  el.innerHTML = `<b>${escapeHtml(who || "??")}</b>: ${escapeHtml(text)}`;
  log.appendChild(el);
  log.scrollTop = log.scrollHeight;
}

/* матч: HUD */
function updateHud(s) {
  const el = document.getElementById("hud");
  const meP = S.latest && S.latest.players.find((p) => p.id === S.myId);
  const meCharge = meP ? meP.ult_charge : 0;
  if (!s.scores.length) { el.innerHTML = ""; return; }
  el.innerHTML = s.scores
    .map((sc) => {
      const info = playerInfo(sc.player_id);
      const name = info ? info.name : "?";
      const color = COLORS[((info && info.color) || 0) % COLORS.length];
      const combo = (s.combo.find((c) => c.player_id === sc.player_id) || {}).combo;
      return `<span class="pill" style="border-color:${color}"><b style="color:${color}">${escapeHtml(name)}</b>
        ${sc.score} ${"❤".repeat(sc.lives)}${combo ? ` <span class="combo">×${combo}</span>` : ""}</span>`;
    })
    .join("")
    + (s.events || []).map((ev) => `<span class="pill combo">⚡ ${EVENT_NAMES[ev.event] || "?"} ${ev.remaining_secs > 100 ? "∞" : Math.max(0, Math.round(ev.remaining_secs)) + "с"}</span>`).join("")
    + `<span class="pill muted">ур. ${s.level} · тик ${s.tick}${S.room && S.room.spectators.length ? " · 👀 " + S.room.spectators.length : ""}</span>`
    + (S.latest && (S.latest.fx & 1) ? '<span class="pill combo">❄ ЛЕД</span>' : "")
    + (S.latest && (S.latest.fx & 2) ? '<span class="pill combo">☄ ПРОБОЙ</span>' : "")
    + (S.latest && (S.latest.fx & 4) ? '<span class="pill combo">🧲 МАГНИТ</span>' : "")
    + (meCharge > 0 ? `<button id="btn-ult" class="pill combo" style="cursor:pointer">⚡ ${meCharge} · U</button>` : "");
}

/* матч: итог */
function showOver(m) {
  const box = document.getElementById("over");
  const reason = m.reason === 1 ? t("winTitle") : m.reason === 2 ? t("exhausted") : t("loseTitle");
  // удержание на экране итогов: комната уже в лобби, но мы не прыгаем мгновенно
  S.overUntil = performance.now() + 3600;
  // тематическое конфетти по цветам участников
  const confColors = (m.scores || []).map((s0) => COLORS[((playerInfo(s0.player_id) || {}).color || 0) % COLORS.length]);
  for (let i = 0; i < 5; i++) {
    const cx = (S.latest ? S.latest.field_w : 800) * (0.2 + 0.15 * i);
    setTimeout(() => spawnFirework(cx, (S.latest ? S.latest.field_h : 600) * (0.25 + 0.1 * ((i % 3))), confColors[i % Math.max(1, confColors.length)] || 0xff9ff3, null), i * 260);
  }
  // ачивки: MVP, лучшая серия, клейщик, охотник на боссов
  const best = (key) => Math.max(0, ...m.scores.map((s) => s[key] || 0));
  const maxCombo = best("max_combo"), maxBricks = best("bricks"), maxBoss = best("boss_hits");
  const medals = (s) => {
    let out = "";
    if (s.place === 1 && m.scores.length > 1) out += " 👑MVP";
    if (s.max_combo >= 5 && s.max_combo === maxCombo) out += ` 🎯×${s.max_combo}`;
    if (s.bricks > 0 && s.bricks === maxBricks) out += ` 🧱${s.bricks}`;
    if (s.boss_hits > 0 && s.boss_hits === maxBoss) out += ` 🐙${s.boss_hits}`;
    return out;
  };
  box.innerHTML = `<h2>${reason}</h2>` +
    m.scores.map((s) => `<div>${s.place}. <b>${escapeHtml(s.name)}</b> — ${s.score}${medals(s)}</div>`).join("") +
    `<p class='muted'>${t("backToLobby")}</p>`;
  box.style.display = "block";
}

/* ---------- PixiJS рендер ---------- */
let app, g;
async function initPixi() {
  app = new PIXI.Application();
  await app.init({ width: 800, height: 600, backgroundAlpha: 0, antialias: true, autoDensity: true, preserveDrawingBuffer: true });
  const holder = document.getElementById("canvas-holder");
  holder.appendChild(app.canvas);
  window.__pixiApp = app; // для отладки/тестов
  window.__S = S;
  // слои: фон-кэш -> кирпичи-кэш -> динамика -> всплывающие тексты
  window.__bgG = new PIXI.Graphics();
  window.__brickG = new PIXI.Graphics();
  app.stage.addChild(window.__bgG);
  app.stage.addChild(window.__brickG);
  g = new PIXI.Graphics();
  app.stage.addChild(g);
  app.ticker.add(() => {
    render();
    updateParticles(app.ticker.deltaMS / 1000);
    // FPS-монитор: ниже 45 два тика подряд — отключаем частицы (автодеградация FX)
    S.fpsFrames++; S.fpsAcc += app.ticker.deltaMS;
    if (S.fpsAcc >= 1000) {
      S.fps = S.fpsFrames * 1000 / S.fpsAcc;
      S.fpsFrames = 0; S.fpsAcc = 0;
      if (S.fps < 45) S.particlesOn = false;
    }
  });
  fitCanvas(800);
}

function fitCanvas(fieldW) {
  S.bgDirty = true; S.bricksDirty = true;
  const holder = document.getElementById("canvas-holder");
  const maxW = Math.min(holder.clientWidth || 860, 960);
  const scale = Math.min(1, maxW / fieldW);
  app.renderer.resize(fieldW, S.fieldH || 600);
  // КЛЮЧЕВОЕ: высота обязана масштабироваться вместе с шириной,
  // иначе широкий мир (10 игроков) сплющивается по горизонтали
  app.canvas.style.width = Math.round(fieldW * scale) + "px";
  app.canvas.style.height = Math.round((S.fieldH || 600) * scale) + "px";
}

function lerp(a, b, k) { return a + (b - a) * k; }

/* ---------- SFX: событийная аудио-модель, чистый WebAudio-синтез ---------- */
const SFX = { ctx: null };
function sfxCtx() {
  if (!SFX.ctx) {
    try { SFX.ctx = new (window.AudioContext || window.webkitAudioContext)(); } catch (e) { return null; }
  }
  if (SFX.ctx.state === "suspended") SFX.ctx.resume();
  return SFX.ctx;
}
function tone(freq, dur, type, gain, slideTo) {
  if (!MUSIC.on) return;
  const c = sfxCtx(); if (!c) return;
  const o = c.createOscillator(), gn = c.createGain();
  o.type = type; o.frequency.setValueAtTime(freq, c.currentTime);
  if (slideTo) o.frequency.exponentialRampToValueAtTime(slideTo, c.currentTime + dur);
  gn.gain.setValueAtTime(gain, c.currentTime);
  gn.gain.exponentialRampToValueAtTime(0.001, c.currentTime + dur);
  o.connect(gn); gn.connect(c.destination);
  o.start(); o.stop(c.currentTime + dur + 0.05);
}
function noiseBurst(dur, gain) {
  if (!MUSIC.on) return;
  const c = sfxCtx(); if (!c) return;
  const n = Math.floor(c.sampleRate * dur);
  const buf = c.createBuffer(1, n, c.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / n);
  const src = c.createBufferSource(); src.buffer = buf;
  const gn = c.createGain(); gn.gain.value = gain;
  const fl = c.createBiquadFilter(); fl.type = "highpass"; fl.frequency.value = 900;
  src.connect(fl); fl.connect(gn); gn.connect(c.destination);
  src.start();
}
const sfxPaddle = () => tone(190, 0.07, "square", 0.045, 320);
const sfxBrick = () => { noiseBurst(0.06, 0.05); tone(520 + Math.random() * 260, 0.05, "triangle", 0.028); };
const sfxCapsule = () => { tone(523, 0.09, "sine", 0.05); setTimeout(() => tone(784, 0.12, "sine", 0.05), 70); };
const sfxLaser = () => tone(980, 0.09, "sawtooth", 0.022, 320);
const sfxBossHit = () => { tone(110, 0.16, "sine", 0.09, 55); noiseBurst(0.09, 0.045); };
const sfxLose = () => tone(330, 0.4, "sawtooth", 0.05, 90);
const sfxFirework = () => { noiseBurst(0.12, 0.04); tone(660, 0.2, "sine", 0.035, 1320); };
const sfxShieldHit = () => { tone(300, 0.1, "square", 0.04, 150); noiseBurst(0.05, 0.03); };
const sfxPoof = () => { noiseBurst(0.14, 0.05); tone(220, 0.12, "sine", 0.035, 90); };
const sfxLaunch = () => { tone(420, 0.08, "triangle", 0.03, 880); };

/* ---------- частицы (локальный сок, автоотключение при низком FPS) ---------- */
const PARTICLE_CAP = 150;
function spawnRing(x, y, color, maxR) {
  if (!Number.isFinite(color)) color = 0xffffff;
  if (!S.particlesOn) return;
  if (S.particles.length >= PARTICLE_CAP) S.particles.shift();
  S.particles.push({ ring: true, x, y, t: 0, life: 0.38, color, maxR: maxR || 30 });
}
function spawnFirework(x, y, color, label) {
  if (!Number.isFinite(color)) color = 0xff9ff3;
  if (S.particlesOn) {
    for (let i = 0; i < 26; i++) {
      const a = Math.random() * Math.PI * 2, v = 90 + Math.random() * 220;
      S.particles.push({ x, y, vx: Math.cos(a) * v, vy: Math.sin(a) * v - 80, t: 0, life: 0.6 + Math.random() * 0.5, size: 2 + Math.random() * 3.5 });
    }
    spawnRing(x, y, color, 70);
  }
  sfxFirework();
  if (label) floatText(x, y - 40, label, color);
}
function floatText(x, y, text, color) {
  if (!Number.isFinite(color)) color = 0xffffff;
  const obj = new PIXI.Text({
    text,
    style: { fontFamily: "Comic Sans MS, sans-serif", fontSize: 26, fontWeight: "bold", fill: color, stroke: { color: 0xffffff, width: 4 } },
  });
  obj.anchor.set(0.5);
  obj.x = x; obj.y = y;
  app.stage.addChild(obj);
  S.floatTexts.push({ obj, x, y, t: 0, life: 1.1 });
}
function spawnBurst(c, r) {
  const cellw = S.latest.field_w / 10;
  const x = c * cellw + cellw / 2;
  const y = GRID_TOP + r * CELL_H + 13;
  const n = 8;
  for (let i = 0; i < n; i++) {
    if (S.particles.length >= PARTICLE_CAP) S.particles.shift();
    const a = Math.random() * Math.PI * 2;
    const v = 60 + Math.random() * 160;
    S.particles.push({ x, y, vx: Math.cos(a) * v, vy: Math.sin(a) * v - 60, t: 0, life: 0.4 + Math.random() * 0.4, size: 2 + Math.random() * 3 });
  }
}
function updateParticles(dt) {
  for (const p of S.particles) {
    p.t += dt;
    if (!p.ring) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 500 * dt; }
  }
  S.particles = S.particles.filter((p) => p.t < p.life);
  for (const f of S.floatTexts) {
    f.t += dt;
    f.obj.y = f.y - f.t * 46;
    f.obj.alpha = Math.max(0, 1 - f.t / f.life);
  }
  for (const f of S.floatTexts.filter((f) => f.t >= f.life)) f.obj.destroy();
  S.floatTexts = S.floatTexts.filter((f) => f.t < f.life);
}
function drawParticles() {
  for (const p of S.particles) {
    const a = 1 - p.t / p.life;
    if (p.ring) {
      const rc = Number.isFinite(p.color) ? p.color : 0xffffff;
      g.circle(p.x, p.y, p.maxR * (p.t / p.life)).stroke({ color: rc, width: 3, alpha: a * 0.9 });
    } else {
      g.circle(p.x, p.y, p.size * a).fill({ color: 0xff9ff3, alpha: a });
    }
  }
}

// пара снапшотов вокруг renderTime + коэффициент (для лерпа списков без id)
function interpPair(buf, renderTime, get) {
  if (buf.length < 2) return [null, null, 0];
  let a = null, b = null;
  for (const s of buf) {
    if (s.recv <= renderTime) a = s;
    else { b = s; break; }
  }
  if (!a) a = buf[0];
  if (!b) b = buf[buf.length - 1];
  const k = Math.min(1, Math.max(0, (renderTime - a.recv) / Math.max(1, b.recv - a.recv)));
  return [get(a.snap), get(b.snap), k];
}
// интерполяция одиночной сущности (босс): Эрмит по снапшотам вокруг renderTime
function entityAtObj(buf, renderTime, get) {
  if (buf.length < 2) { const s0 = buf[0]; return s0 ? get(s0.snap) : null; }
  let ia = 0;
  for (let i = 0; i < buf.length; i++) {
    if (buf[i].recv <= renderTime) ia = i; else break;
  }
  const a = buf[ia], b = buf[Math.min(buf.length - 1, ia + 1)];
  const ea = get(a.snap), eb = get(b.snap);
  if (!ea) return eb;
  if (!eb || ea === eb) return ea;
  const span = Math.max(1, b.recv - a.recv);
  const t = Math.min(1, Math.max(0, (renderTime - a.recv) / span));
  const prevBuf = ia > 0 ? buf[ia - 1] : null;
  const nextBuf = ia + 2 < buf.length ? buf[ia + 2] : null;
  const herm = (xa, xb, va, vb) => {
    const t2 = t * t, t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1, h10 = t3 - 2 * t2 + t;
    const h01 = -2 * t3 + 3 * t2, h11 = t3 - t2;
    let v = h00 * xa + h10 * span * va + h01 * xb + h11 * span * vb;
    const lin = xa + (xb - xa) * t;
    if (Math.abs(v - lin) > 9) v = lin;
    return v;
  };
  const dxa = prevBuf ? (eb.x - get(prevBuf.snap).x) / Math.max(1, b.recv - prevBuf.recv) : (eb.x - ea.x) / span;
  const dxb = nextBuf ? (get(nextBuf.snap).x - ea.x) / Math.max(1, nextBuf.recv - a.recv) : (eb.x - ea.x) / span;
  const dya = prevBuf ? (eb.y - get(prevBuf.snap).y) / Math.max(1, b.recv - prevBuf.recv) : (eb.y - ea.y) / span;
  const dyb = nextBuf ? (get(nextBuf.snap).y - ea.y) / Math.max(1, nextBuf.recv - a.recv) : (eb.y - ea.y) / span;
  return { ...eb, x: herm(ea.x, eb.x, dxa, dxb), y: herm(ea.y, eb.y, dya, dyb) };
}

function entityAt(buf, renderTime, id, get) {
  // Кубический Эрмит по двум снапшотам вокруг renderTime; наклоны — конечные
  // разности по соседям: траектория C1-гладкая, без изломов на стыках снапшотов.
  if (buf.length < 2) { const s0 = buf[0]; return s0 ? get(s0.snap) : null; }
  let ia = 0;
  for (let i = 0; i < buf.length; i++) {
    if (buf[i].recv <= renderTime) ia = i; else break;
  }
  const a = buf[ia], b = buf[Math.min(buf.length - 1, ia + 1)];
  const ea = get(a.snap), eb = get(b.snap);
  if (!ea) return eb;
  if (!eb || ea === eb) return ea;
  const span = Math.max(1, b.recv - a.recv);
  const t = Math.min(1, Math.max(0, (renderTime - a.recv) / span));
  const prevBuf = ia > 0 ? buf[ia - 1] : null;
  const nextBuf = ia + 2 < buf.length ? buf[ia + 2] : null;
  const prevA = prevBuf ? get(prevBuf.snap) : null;
  const nextB = nextBuf ? get(nextBuf.snap) : null;
  const herm = (xa, xb, va, vb) => {
    const t2 = t * t, t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1, h10 = t3 - 2 * t2 + t;
    const h01 = -2 * t3 + 3 * t2, h11 = t3 - t2;
    let v = h00 * xa + h10 * span * va + h01 * xb + h11 * span * vb;
    // страховка от выброса на резком отскоке: не дальше 9px от линейного
    const lin = xa + (xb - xa) * t;
    if (Math.abs(v - lin) > 9) v = lin;
    return v;
  };
  const dxa = prevA ? (eb.x - prevA.x) / Math.max(1, b.recv - prevBuf.recv) : (eb.x - ea.x) / span;
  const dxb = nextB ? (nextB.x - ea.x) / Math.max(1, nextBuf.recv - a.recv) : (eb.x - ea.x) / span;
  const dya = prevA ? (eb.y - prevA.y) / Math.max(1, b.recv - prevBuf.recv) : (eb.y - ea.y) / span;
  const dyb = nextB ? (nextB.y - ea.y) / Math.max(1, nextBuf.recv - a.recv) : (eb.y - ea.y) / span;
  return { ...eb, x: herm(ea.x, eb.x, dxa, dxb), y: herm(ea.y, eb.y, dya, dyb) };
}

// детерминированный рандом для фона (одинаковое поле у всех)
function bgRand(seedRef) {
  seedRef.v = (seedRef.v ^ (seedRef.v << 13)) >>> 0;
  seedRef.v = (seedRef.v ^ (seedRef.v >>> 7)) >>> 0;
  seedRef.v = (seedRef.v ^ (seedRef.v << 17)) >>> 0;
  return seedRef.v / 4294967296;
}
const BG_PASTEL = [0xffd1dc, 0xffe9b3, 0xc9f0ff, 0xd9ffd6, 0xe8d9ff, 0xffd9ec];
function drawBackgroundCache(W, H) {
  // редкая перерисовка: градиент + сетка (цвет сетки зависит от наличия босса)
  const bgG = window.__bgG;
  bgG.clear();
  const bossNow = !!(S.latest && S.latest.boss);
  bgG.rect(0, 0, W, H).fill({ color: 0xfff4ec, alpha: 0.9 });
  bgG.rect(0, 0, W, H * 0.55).fill({ color: 0xffe6f0, alpha: 0.55 });
  bgG.rect(0, H * 0.45, W, H * 0.55).fill({ color: 0xe3f4ff, alpha: 0.5 });
  const gridC = bossNow ? 0xff6b6b : 0x8e7cf3;
  for (let x = 80; x < W; x += 80) bgG.rect(x - 1, 0, 2, H).fill({ color: gridC, alpha: 0.05 });
  for (let y = 80; y < H; y += 80) bgG.rect(0, y - 1, W, 2).fill({ color: gridC, alpha: 0.05 });
  S.bgBoss = bossNow;
  S.bgDirty = false;
}
function drawBubbles(W, H) {
  // динамичные пузыри — каждый кадр, но их всего 32
  const time = performance.now() / 1000;
  const seed = { v: S.bgSeed };
  for (let layer = 0; layer < 2; layer++) {
    const speed = layer === 0 ? 9 : 16;
    const scale = layer === 0 ? 1 : 1.5;
    for (let i = 0; i < 16; i++) {
      const bx = bgRand(seed) * W;
      const by0 = bgRand(seed) * (H + 120);
      const r = (2 + bgRand(seed) * 4) * scale;
      const col = BG_PASTEL[Math.floor(bgRand(seed) * BG_PASTEL.length)];
      const by = (by0 - time * speed) % (H + 120);
      const y = by < -60 ? by + H + 120 : by;
      g.circle(bx, y, r).fill({ color: col, alpha: layer === 0 ? 0.35 : 0.22 });
      g.circle(bx - r * 0.3, y - r * 0.3, r * 0.35).fill({ color: 0xffffff, alpha: 0.5 });
    }
  }
}

const BRICK_KIND_COLORS = {
  1: [0xff9f43, 0xee5253], // взрывной
  2: [0xa55eea, 0x8854d0], // телепорт
  3: [0x2ecc71, 0x27ae60], // регенерирующий
  4: [0x48dbfb, 0x0abde3], // движущийся
};
function drawBricksCache(W) {
  const bg = window.__brickG;
  bg.clear();
  for (const b of S.bricks.values()) {
    if ((b.kind || 0) === 4) continue; // движущиеся рисуются покадрово
    const cellw = W / 10;
    let x = b.c * cellw + 3, y = GRID_TOP + b.r * CELL_H + 3, w = cellw - 6, h = CELL_H - 6;
    const born = S.brickBorn.get(b.id);
    if (born !== undefined) {
      const age = (performance.now() - born) / 260;
      if (age >= 1) S.brickBorn.delete(b.id);
      else {
        const sc = (0.25 + 0.75 * age) * (1 + 0.22 * Math.sin(age * Math.PI));
        const cx2 = x + w / 2, cy2 = y + h / 2;
        x = cx2 - (w / 2) * sc; y = cy2 - (h / 2) * sc; w *= sc; h *= sc;
      }
    }
    const kcol = BRICK_KIND_COLORS[b.kind || 0];
    const col = kcol || BRICK_COLORS[b.hp] || BRICK_COLORS[1];
    const maxHp = S.brickMax.get(b.id) || b.hp;
    const cracks = b.hp < 200 ? Math.min(2, maxHp - b.hp) : 0;
    bg.roundRect(x + 2, y + 3, w, h, 7).fill({ color: 0x3b3552, alpha: 0.16 });
    bg.roundRect(x, y, w, h, 7).fill({ color: col[0], alpha: 0.62 });
    bg.roundRect(x + 4, y + 4, w - 8, h - 8, 5).fill({ color: col[1], alpha: 0.32 });
    bg.roundRect(x + 3, y + 2.5, w - 6, 5, 2.5).fill({ color: 0xffffff, alpha: 0.8 });
    bg.circle(x + 7, y + 7, 2.5).fill({ color: 0xffffff, alpha: 0.9 });
    bg.roundRect(x, y, w, h, 7).stroke({ color: 0xffffff, width: 1.5, alpha: 0.55 });
    // маркеры типов
    if (b.kind === 1) { // взрывной: звезда
      const cx2 = x + w / 2, cy2 = y + h / 2, rr = Math.min(w, h) * 0.26;
      bg.moveTo(cx2 + rr, cy2);
      for (let k2 = 1; k2 < 9; k2++) {
        const ang = (k2 * Math.PI) / 4, rad = k2 % 2 ? rr * 0.45 : rr;
        bg.lineTo(cx2 + Math.cos(ang) * rad, cy2 + Math.sin(ang) * rad);
      }
      bg.closePath();
      bg.fill({ color: 0x2f2b44, alpha: 0.75 });
    } else if (b.kind === 2) { // телепорт: спираль
      const cx2 = x + w / 2, cy2 = y + h / 2;
      bg.moveTo(cx2 + 6, cy2);
      bg.arc(cx2, cy2, 6, 0, Math.PI * 1.4);
      bg.moveTo(cx2 - 6, cy2);
      bg.arc(cx2, cy2, 6, Math.PI, Math.PI * 2.4);
      bg.stroke({ color: 0xffffff, width: 2, alpha: 0.85 });
    } else if (b.kind === 3) { // реген: крест + пипсы hp
      const cx2 = x + w / 2, cy2 = y + h / 2;
      bg.moveTo(cx2 - 5, cy2); bg.lineTo(cx2 + 5, cy2);
      bg.moveTo(cx2, cy2 - 5); bg.lineTo(cx2, cy2 + 5);
      bg.stroke({ color: 0xffffff, width: 2.4, alpha: 0.9 });
      for (let k2 = 0; k2 < (maxHp || 1); k2++) {
        bg.circle(x + 6 + k2 * 6, y + h - 5, 2).fill({ color: k2 < b.hp ? 0xd1f2e0 : 0x2f2b44, alpha: 0.95 });
      }
    }
    for (let k = 0; k < cracks; k++) {
      let cx = x + 8 + ((b.id * 7 + k * 29) % Math.max(1, w - 16));
      let cy = y + 4 + ((b.id * 13 + k * 7) % Math.max(1, h - 8));
      bg.moveTo(cx, cy);
      for (let j = 0; j < 3; j++) {
        cx += ((b.id * 11 + k * 17 + j * 31) % 15) - 7;
        cy += ((b.id * 19 + k * 23 + j * 13) % 9) - 3;
        bg.lineTo(cx, cy);
      }
      bg.stroke({ color: 0x3b3552, width: 1.4, alpha: 0.5 });
    }
  }
  S.bricksDirty = false;
}

/* движущиеся кирпичи + эффекты ульт поверх поля */
function drawUltFx(W, H) {
  if (!S.latest || !S.latest.fx) return;
  const t = performance.now() / 1000;
  if (S.latest.fx & 1) { // лед: холодная подложка поля
    g.roundRect(0, GRID_TOP - 6, W, H - GRID_TOP + 6, 12).stroke({ color: 0x81d4fa, width: 3, alpha: 0.35 + 0.1 * Math.sin(t * 4) });
    for (let i = 0; i < 8; i++) {
      const sx = ((i * 137 + (t * 40) % W) % W);
      const sy = GRID_TOP + ((i * 61 + (t * 30) % (H - GRID_TOP)) % (H - GRID_TOP));
      g.circle(sx, sy, 1.8).fill({ color: 0xd1f0ff, alpha: 0.5 });
    }
  }
}
/* движущиеся кирпичи: смещение = серверная формула от тика снапшота */
function drawMovingBricks(W) {
  if (!S.latest) return;
  const cellw = W / 10;
  const tick = S.latest.tick || 0;
  for (const b of S.bricks.values()) {
    if ((b.kind || 0) !== 4) continue;
    const dx = Math.sin(tick * 0.55 + b.r * 7.3) * cellw * 0.38;
    const x = b.c * cellw + 3 + dx, y = GRID_TOP + b.r * CELL_H + 3, w = cellw - 6, h = CELL_H - 6;
    const col = BRICK_KIND_COLORS[4];
    g.roundRect(x + 2, y + 3, w, h, 7).fill({ color: 0x3b3552, alpha: 0.16 });
    g.roundRect(x, y, w, h, 7).fill({ color: col[0], alpha: 0.62 });
    g.roundRect(x + 4, y + 4, w - 8, h - 8, 5).fill({ color: col[1], alpha: 0.32 });
    g.roundRect(x + 3, y + 2.5, w - 6, 5, 2.5).fill({ color: 0xffffff, alpha: 0.8 });
    g.roundRect(x, y, w, h, 7).stroke({ color: 0xffffff, width: 1.5, alpha: 0.55 });
    g.moveTo(x + w / 2 - 7, y + h / 2); g.lineTo(x + w / 2 + 7, y + h / 2);
    g.lineTo(x + w / 2 + 3, y + h / 2 - 4);
    g.moveTo(x + w / 2 + 7, y + h / 2); g.lineTo(x + w / 2 - 7, y + h / 2);
    g.lineTo(x + w / 2 - 3, y + h / 2 + 4);
    g.stroke({ color: 0x2f2b44, width: 2, alpha: 0.8, cap: "round", join: "round" });
  }
}

function render() {
  if (!g) return;
  g.clear();
  if (!S.latest) { drawIdleTitle(); return; }
  const delay = Math.min(140, Math.max(50, (S.snapInt || 100) * 1.6));
  const rt = performance.now() - delay;
  const buf = S.snapBuf.length ? S.snapBuf : [{ recv: 0, snap: S.latest }];
  const W = S.latest.field_w, H = S.latest.field_h;

  const bossNow = !!S.latest.boss;
  if (S.bgDirty || bossNow !== S.bgBoss) drawBackgroundCache(W, H);
  if (S.bricksDirty || S.brickBorn.size > 0) drawBricksCache(W);
  drawMovingBricks(W);
  drawUltFx(W, H);
  drawBubbles(W, H);

  if ((S.latest.events || []).some((ev) => ev.event === 1)) {
    g.rect(0, 0, W, H).fill({ color: 0x1a1530, alpha: 0.78 });
  }
  if ((S.latest.events || []).some((ev) => ev.event === 8)) {
    g.rect(0, H - 20, W, 5).fill(0x1dd1a1);
  }

  const boss = S.latest.boss;
  if (boss) {
    const be = entityAtObj(buf, rt, (s0) => s0.boss);
    const bx = be ? be.x : boss.x, by = be ? be.y : boss.y;
    const KIND_COLORS = [[0x8e7cf3, 0x5a3fd6], [0x7ed6f5, 0x2b7bb5], [0xffb03a, 0xf0640f]];
    const [bc1, bc2] = KIND_COLORS[boss.kind] || KIND_COLORS[0];
    const time = performance.now() / 1000;
    // ---- тематический босс-бар: крупный, скруглённый, с именем-волной ----
    const bw = W * 0.72, bh = 20, bx0 = (W - bw) / 2, by0 = 16;
    const frac = Math.max(0, boss.hp / boss.max_hp);
    // подложка + рамка
    g.roundRect(bx0 - 3, by0 - 3, bw + 6, bh + 6, 13).fill({ color: 0x2f2b44, alpha: 0.55 });
    g.roundRect(bx0, by0, bw, bh, 10).fill({ color: 0x241f38, alpha: 0.95 });
    // полоса HP: цвет плавно от зелёного к красному по остатку
    const hf = Math.max(0, Math.min(1, frac));
    const hpColor = hf > 0.5 ? 0x1dd1a1 : hf > 0.25 ? 0xfeca57 : 0xff6b6b;
    if (frac > 0.01) {
      g.roundRect(bx0 + 3, by0 + 3, Math.max(6, (bw - 6) * frac), bh - 6, 7).fill(hpColor);
      // бегущий блик по полосе
      const sheenX = bx0 + 3 + ((time * 140) % Math.max(1, (bw - 6) * frac));
      g.roundRect(sheenX - 14, by0 + 4, 12, bh - 8, 5).fill({ color: 0xffffff, alpha: 0.22 });
    }
    g.roundRect(bx0, by0, bw, bh, 10).stroke({ color: 0xffffff, width: 2, alpha: 0.85 });
    // имя босса волной (по символам, спавн при появлении босса)
    const KIND_NAMES_RU = ["КРАКЕН", "ЛЕДЯНОЙ ГОЛЕТ", "МЕГА-КУБИК"];
    const KIND_NAMES_EN = ["KRAKEN", "ICE GOLEM", "MEGA CUBE"];
    const KIND_NAME_COLORS = [0xb39ddb, 0x81d4fa, 0xffcc80];
    const nameText = (LANG === "en" ? KIND_NAMES_EN : KIND_NAMES_RU)[boss.kind] || "BOSS";
    if (!window.__bossName || window.__bossName.kind !== boss.kind || window.__bossName.text !== nameText) {
      if (window.__bossName) window.__bossName.chars.forEach((c) => c.destroy());
      const chars = nameText.split("").map((ch) => {
        const t2 = new PIXI.Text({
          text: ch === " " ? " " : ch,
          style: { fontFamily: "Comic Sans MS, sans-serif", fontSize: 20, fontWeight: "bold",
            fill: KIND_NAME_COLORS[boss.kind] || 0xffffff, stroke: { color: 0x2f2b44, width: 4 } },
        });
        t2.anchor.set(0.5);
        app.stage.addChild(t2);
        return t2;
      });
      window.__bossName = { kind: boss.kind, text: nameText, chars };
    }
    const bn = window.__bossName.chars;
    const spacing = 15;
    for (let i = 0; i < bn.length; i++) {
      bn[i].x = W / 2 + (i - (bn.length - 1) / 2) * spacing;
      bn[i].y = by0 + bh + 18 + Math.sin(time * 3 - i * 0.55) * 3.5;
      bn[i].alpha = 0.85 + 0.15 * Math.sin(time * 3 - i * 0.55);
    }
    // ---- щиты: орбиты-щупальца с серверными углами и видимым HP ----
    const shList = boss.shields || [];
    if (shList.length > 0) {
      g.circle(bx, by, 60).stroke({ color: bc1, width: 1.5, alpha: 0.22 });
      const prevHps = S.prevShieldHps || [];
      shList.forEach((sh, i) => {
        const ang = (sh.angle * 1.40625 * Math.PI) / 180;
        const ox = bx + Math.cos(ang) * 60, oy = by + Math.sin(ang) * 60;
        const hpFrac = Math.max(0, Math.min(1, sh.hp / 4));
        // орб тускнеет и мельчает по мере урона
        const orbR = 9 + 5 * hpFrac;
        g.circle(ox, oy, orbR).fill({ color: bc1, alpha: 0.45 + 0.4 * hpFrac });
        g.circle(ox, oy, orbR).stroke({ color: 0xffffff, width: 2, alpha: 0.9 });
        g.circle(ox - 3, oy - 3, 2.5).fill({ color: 0xffffff, alpha: 0.8 });
        // трещины: штрихи по числу потерянных HP
        const dmg = 4 - sh.hp;
        for (let k = 0; k < dmg; k++) {
          const ca = ang + k * 1.1;
          g.moveTo(ox + Math.cos(ca) * orbR * 0.3, oy + Math.sin(ca) * orbR * 0.3);
          g.lineTo(ox + Math.cos(ca + 0.5) * orbR, oy + Math.sin(ca + 0.5) * orbR);
          g.stroke({ color: 0x2f2b44, width: 1.6, alpha: 0.7 });
        }
        // вспышка при новом уроне по этому щиту
        if (prevHps[i] !== undefined && sh.hp < prevHps[i]) {
          spawnRing(ox, oy, 0xffffff, 26);
          sfxShieldHit();
        }
      });
      S.prevShieldHps = shList.map((sh) => sh.hp);
    } else {
      S.prevShieldHps = [];
    }
    // ---- тело ----
    const pulse = boss.phase ? 2.6 * Math.sin(time * 10) : 1.4 * Math.sin(time * 3);
    if (boss.phase) g.circle(bx, by, 52 + 4 * Math.sin(time * 10)).fill({ color: 0xff6b6b, alpha: 0.22 });
    g.circle(bx, by, 42).fill(bc2);
    g.circle(bx, by, 30 + pulse).fill(boss.phase ? 0xff6b6b : bc1);
    // ---- снаряды: интерполяция по id ----
    for (const p of S.latest.projs || []) {
      const e = entityAt(buf, rt, p.id, (s) => s.projs.find((q) => q.id === p.id));
      if (!e) continue;
      g.circle(e.x, e.y, 6).fill(0xe0181d);
      g.circle(e.x, e.y, 3).fill(0xffffff);
    }
  } else if (window.__bossName) {
    window.__bossName.chars.forEach((c) => c.destroy());
    window.__bossName = null;
  }

  const CAPSULE_COLORS = [0xe84393, 0x10ac84, 0xee7724, 0x4a69bd, 0xe0181d];
  for (const cp of S.latest.capsules || []) {
    const e = entityAt(buf, rt, cp.id, (s) => s.capsules.find((q) => q.id === cp.id));
    if (!e) continue;
    g.roundRect(e.x - 13, e.y - 11, 26, 22, 8).fill(CAPSULE_COLORS[cp.kind] || 0xfeca57);
    if (S.latest && (S.latest.fx & 4)) {
      g.circle(e.x, e.y, 20 + 2.5 * Math.sin(performance.now() / 100)).stroke({ color: 0x81d4fa, width: 2, alpha: 0.55 });
    }
    g.roundRect(e.x - 13, e.y - 11, 26, 22, 8).stroke({ color: 0xffffff, width: 2, alpha: 0.9 });
  }
  for (const u of S.latest.bullets || []) {
    const e = entityAt(buf, rt, u.id, (s) => s.bullets.find((q) => q.id === u.id));
    if (!e) continue;
    g.roundRect(e.x - 2.5, e.y - 12, 5, 14, 2.5).fill(0xee7724);
  }

  const nowMs = performance.now();
  const dtS = Math.min(0.05, (nowMs - (S.lastFrame || nowMs)) / 1000);
  S.lastFrame = nowMs;
  for (const p of S.latest.players) {
    if (p.state === 1) continue; // Spectator: умер — ракетка покидает поле
    let e = entityAt(buf, rt, p.id, (s) => s.players.find((q) => q.id === p.id));
    if (!e) continue;
    if (p.id === S.myId && S.paddleX !== null && p.state !== 2) {
      const half = e.w / 2;
      const target = Math.min(S.latest.field_w - half, Math.max(half, S.inputX));
      S.paddleX += Math.max(-760 * dtS, Math.min(760 * dtS, target - S.paddleX));
      e = { ...e, x: S.paddleX };
    }
    const base = COLORS[((playerInfo(p.id) || {}).color || 0) % COLORS.length];
    const color = p.state === 2 ? 0x9aa5b1 : base;
    const py = H - 42;
    g.roundRect(e.x - e.w / 2, py, e.w, PADDLE_H, PADDLE_H / 2).fill(color);
    g.roundRect(e.x - e.w / 2 + 4, py + 2, e.w - 8, 4, 2).fill({ color: 0xffffff, alpha: 0.5 });
  }

  // хвосты живут только у мячей в поле; мёртвые удаляем, телепорт не сшиваем
  const liveIds = new Set(S.latest.balls.map((b) => b.id));
  for (const id of [...S.trails.keys()]) if (!liveIds.has(id)) S.trails.delete(id);
  const popNow = performance.now();
  if (!S.ballSpawn) S.ballSpawn = new Map();
  if (!S.prevBalls) S.prevBalls = new Map();
  const curBalls = new Map();
  for (const b of S.latest.balls) {
    const e = entityAt(buf, rt, b.id, (s) => s.balls.find((q) => q.id === b.id));
    if (!e) { S.trails.delete(b.id); continue; }
    // ушедший за нижнюю кромку мяч не рисуем: без «призрака» под ракеткой
    if (e.y > H + 10) { S.trails.delete(b.id); continue; }
    const color = S.latest && (S.latest.fx & 2) ? 0xff9f43 : pcolor((playerInfo(b.owner_id) || {}).color);
    curBalls.set(b.id, { x: e.x, y: e.y, color });
    // мой прилипший мяч клеится к предсказанной ракетке — ноль отставания
    const glued = b.owner_id === S.myId && e.y > H - 80 && S.paddleX !== null;
    if (glued) e.x = S.paddleX;
    let tr = S.trails.get(b.id);
    if (!tr) { tr = []; S.trails.set(b.id, tr); }
    const last = tr[tr.length - 1];
    // прычок больше поля = respawn/телепорт: хвост обнуляем, а не тянем через весь экран
    if (last && Math.hypot(e.x - last.x, e.y - last.y) > 140) tr.length = 0;
    // прилипший мяч хвоста не оставляет
    if (!glued) {
      tr.push({ x: e.x, y: e.y });
      if (tr.length > 8) tr.shift();
      if (tr.length > 1) {
        g.moveTo(tr[0].x, tr[0].y);
        for (let i = 1; i < tr.length - 1; i++) g.lineTo(tr[i].x, tr[i].y);
        g.stroke({ color, width: BALL_R * 1.1, alpha: 0.25, cap: "round", join: "round" });
      }
    } else if (tr.length) tr.length = 0;
    // pop-in: новый мяч появляется у ракетки с «выпрыгиванием»
    if (!S.ballSpawn.has(b.id)) {
      S.ballSpawn.set(b.id, popNow);
      if (b.owner_id === S.myId) sfxLaunch();
    }
    b._pop = Math.min(1, (popNow - S.ballSpawn.get(b.id)) / 220);
    g.circle(e.x, e.y, BALL_R * 1.9).fill({ color, alpha: 0.16 });
    g.circle(e.x, e.y, BALL_R * 1.35).fill({ color, alpha: 0.22 });
    g.circle(e.x, e.y, BALL_R * (0.4 + 0.6 * (b._pop === undefined ? 1 : b._pop))).fill({ color: 0xffffff, alpha: 0.95 });
    g.circle(e.x, e.y, BALL_R - 2.2).fill(color);
    g.circle(e.x + 1.5, e.y + 2.5, BALL_R - 4).fill({ color: 0x000000, alpha: 0.18 });
    g.circle(e.x - 3, e.y - 3.5, 3).fill({ color: 0xffffff, alpha: 0.95 });
    g.circle(e.x - 1.2, e.y - 1.8, 1.4).fill({ color: 0xffffff, alpha: 0.7 });
  }
  // хлопок на потере мяча: кольцо в точке, где он ушёл за ракетку
  if (S.prevBalls && S.prevBalls.size) {
    for (const [pid, p] of S.prevBalls) {
      if (!curBalls.has(pid)) {
        spawnRing(p.x, Math.min(H - 10, p.y), p.color, 22);
        sfxPoof();
      }
    }
  }
  S.prevBalls = curBalls;

  if (S.trails.size > S.latest.balls.length * 2) {
    const ids = new Set(S.latest.balls.map((b) => b.id));
    for (const id of S.trails.keys()) if (!ids.has(id)) S.trails.delete(id);
  }

  drawParticles();
}

function drawIdleTitle() {
  // Заглушка до первого снапшота
}

/* ---------- ввод ---------- */
function bindInput() {
  const canvas = () => document.querySelector("#canvas-holder canvas");
  document.addEventListener("pointermove", (e) => {
    const c = canvas();
    if (!c || !S.latest || document.getElementById("screen-game").style.display === "none") return;
    const rect = c.getBoundingClientRect();
    if (!rect.width) return;
    S.inputX = (e.clientX - rect.left) / rect.width * S.latest.field_w;
    throttledPaddle();
  });
  document.addEventListener("pointerdown", (e) => {
    if (!S.latest || document.getElementById("screen-game").style.display === "none") return;
    if (e.target.closest("button")) return;
    send(P.mAction());
  });
  window.addEventListener("keydown", (e) => {
    if (document.getElementById("screen-game").style.display === "none") return;
    if (e.key === " ") { e.preventDefault(); send(P.mAction()); }
    if (e.key >= "1" && e.key <= "8") send(P.mEmote(+e.key - 1));
    if (e.key === "u" || e.key === "U" || e.key === "у" || e.key === "У") send(P.mUseUlt());
  });
  setInterval(() => { if (S.latest) throttledPaddle(); }, 33);
}
function throttledPaddle() {
  const now = performance.now();
  if (now - S.lastSend < 25 || !S.latest) return;
  S.lastSend = now;
  send(P.mPaddleX(S.inputX));
}

/* ---------- утилиты ---------- */
function playerInfo(id) {
  if (!S.room) return null;
  return [...S.room.players, ...S.room.spectators].find((p) => p.id === id) || null;
}
function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
let emoteTimer = null;
function showEmote(playerId, n) {
  const phrases = ["😂", "😭", "👏", "🔥", "😱", "💪", "???", "❤️"];
  const info = playerInfo(playerId);
  toast((info ? info.name : "?") + ": " + (phrases[n] || "👍"));
}

/* ---------- генеративная музыка (WebAudio, без файлов) ---------- */
const MUSIC = { ctx: null, on: false, step: 0, timer: null };
const PENTA = [261.6, 293.7, 329.6, 392.0, 440.0, 523.3, 587.3, 659.3];

function musicNote(freq, dur, gain) {
  const c = MUSIC.ctx;
  const o = c.createOscillator(), gn = c.createGain();
  o.type = "triangle";
  o.frequency.value = freq;
  gn.gain.setValueAtTime(gain, c.currentTime);
  gn.gain.exponentialRampToValueAtTime(0.001, c.currentTime + dur);
  o.connect(gn); gn.connect(c.destination);
  o.start(); o.stop(c.currentTime + dur + 0.05);
}

function musicStep() {
  if (!MUSIC.on) return;
  const inMatch = document.getElementById("screen-game").style.display !== "none";
  const intense = inMatch && (S.latest?.boss || (S.latest?.events || []).length > 0);
  const period = intense ? 140 : 220; // мс на шаг
  // мелодия: пентатоника со случайным блужданием
  MUSIC.step = Math.max(0, Math.min(PENTA.length - 1, MUSIC.step + ((Math.random() * 3) | 0) - 1));
  const note = PENTA[MUSIC.step];
  musicNote(note, intense ? 0.16 : 0.3, intense ? 0.05 : 0.035);
  if (intense && MUSIC.step % 2 === 0) musicNote(note / 2, 0.2, 0.05); // бас
  clearInterval(MUSIC.timer);
  MUSIC.timer = setInterval(musicStep, period);
}

function toggleMusic() {
  const btn = document.getElementById("btn-sound");
  if (!MUSIC.on) {
    MUSIC.ctx = MUSIC.ctx || new (window.AudioContext || window.webkitAudioContext)();
    if (MUSIC.ctx.state === "suspended") MUSIC.ctx.resume();
    MUSIC.on = true;
    btn.textContent = "🔊";
    musicStep();
  } else {
    MUSIC.on = false;
    clearInterval(MUSIC.timer);
    btn.textContent = "🔇";
  }
}

/* ---------- запуск ---------- */
window.addEventListener("DOMContentLoaded", async () => {
  applyLang();
  showLoading(false);
  await initPixi();
  bindInput();
  showScreen("menu");
  connect();

  document.getElementById("btn-create").onclick = () => {
    const name = document.getElementById("room-name").value.trim() || "Комната";
    const pass = document.getElementById("room-pass").value.trim();
    const mode = +document.getElementById("room-mode").value;
    send(P.mCreateRoom(name, pass || null, mode, { max_players: 10, events_enabled: true, bosses_enabled: true, speed_factor: 1.0 }));
  };
  document.getElementById("btn-refresh").onclick = () => send(P.mRoomList());
  const loadTop = async () => {
    const box = document.getElementById("top-list");
    try {
      const top = await fetch("/api/leaderboard").then((r) => r.json());
      box.classList.remove("muted");
      box.innerHTML = top.length
        ? top.map((e, i) => `<div class="room-row"><b>${i + 1}. ${escapeHtml(e.name)}</b><span class="muted">${e.xp} XP · lvl ${Math.floor(e.xp / 100) + 1}</span></div>`).join("")
        : `<span class="muted">${t("topEmpty")}</span>`;
    } catch { box.innerHTML = "<span class='muted'>…</span>"; }
  };
  document.getElementById("btn-leaderboard").onclick = loadTop;
  window.__loadTop = loadTop;
  document.getElementById("btn-lang").onclick = () => {
    LANG = LANG === "ru" ? "en" : "ru";
    localStorage.setItem("pop_lang", LANG);
    applyLang();
  };
  document.getElementById("btn-join-code").onclick = () => {
    const code = document.getElementById("join-code").value.trim();
    const pass = document.getElementById("join-pass").value.trim();
    if (code) send(P.mJoinRoom(code, pass || null));
  };
  const jp = document.getElementById("btn-join-play");
  if (jp) jp.onclick = () => send(P.mJoinRoom(S.room.code, localStorage.getItem("pop_room_pass")));
  document.getElementById("btn-ready").onclick = () => {
    const me = S.room && S.room.players.find((p) => p.id === S.myId);
    send(P.mReady(!(me && me.ready)));
    setTimeout(renderRoom, 150);
  };
  document.getElementById("btn-start").onclick = () => send(P.mStart());
  document.getElementById("btn-leave").onclick = () => {
    send(P.mLeave());
    S.room = null;
    localStorage.removeItem("pop_room_code");
    showScreen("menu");
  };
  document.querySelectorAll("#ult-row .ult-btn").forEach((b) => {
    b.onclick = () => {
      send(P.mSetUlt(+b.dataset.ult));
      setTimeout(renderRoom, 120);
    };
  });
  document.getElementById("hud").addEventListener("click", (e) => {
    if (e.target && e.target.id === "btn-ult") send(P.mUseUlt());
  });
  document.getElementById("btn-exit").onclick = () => {
    send(P.mLeave());
    S.room = null;
    S.overUntil = 0; S.overPending = false;
    localStorage.removeItem("pop_room_code");
    showScreen("menu");
    if (window.__loadTop) window.__loadTop();
  };
  document.getElementById("btn-sound").onclick = toggleMusic;
  document.getElementById("btn-bot-lazy").onclick = () => send(P.mAddBot(0));
  document.getElementById("btn-bot-normal").onclick = () => send(P.mAddBot(1));
  document.getElementById("btn-bot-pro").onclick = () => send(P.mAddBot(2));
  document.getElementById("btn-chat").onclick = sendChat;
  document.getElementById("chat-input").addEventListener("keydown", (e) => { if (e.key === "Enter") sendChat(); });
  function sendChat() {
    const v = document.getElementById("chat-input").value.trim();
    if (v) send(P.mChat(v));
    document.getElementById("chat-input").value = "";
  }
});

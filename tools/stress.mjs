// Стресс-тест PoP - Protocol: N комнат × 10 игроков (хост + 9 ботов-профи).
// Меряет: суммарный трафик по WS, RAM/CPU процесса сервера, число живых комнат.
// Запуск: node tools/stress.mjs [port] [rooms] [seconds]
"use strict";
import { exec } from "node:child_process";

const u32 = (v) => { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b; };
const u16 = (v) => { const b = Buffer.alloc(2); b.writeUInt16LE(v); return b; };
const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v)); return b; };
const f32 = (v) => { const b = Buffer.alloc(4); b.writeFloatLE(v); return b; };
const str = (s) => { const u = Buffer.from(s, "utf8"); return Buffer.concat([u64(u.length), u]); };
const opt = (p) => (p === null ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), p]));

const port = process.argv[2] || "18408";
const ROOMS = parseInt(process.argv[3] || "30", 10);
const SECONDS = parseInt(process.argv[4] || "30", 10);
const BASE = `ws://127.0.0.1:${port}/ws`;

function client(name) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(BASE);
    const c = { ws, name, bytes: 0, msgs: 0, snapCount: 0, roomId: null, phase: "hello" };
    ws.onopen = () => ws.send(Buffer.concat([u32(0), u16(6), str(name), opt(null)]));
    ws.onclose = (e) => { if (c.phase === "hello") c.closedCode = e.code; };
    ws.onmessage = async (ev) => {
      const d = ev.data;
      const buf = Buffer.isBuffer(d) ? d : (d instanceof Blob ? Buffer.from(await d.arrayBuffer()) : Buffer.from(d));
      c.bytes += buf.length;
      c.msgs += 1;
      const tag = buf.readUInt32LE(0);
      if (tag === 0) {
        // Welcome -> create
        ws.send(Buffer.concat([u32(2), str("Стресс-" + name), opt(null), u32(0), Buffer.from([10, 1, 1]), f32(1.0)]));
        c.phase = "created";
      } else if (tag === 4 && c.phase === "created") {
        c.phase = "bots";
        c.roomId = buf.readUInt32LE(4);
        // 9 ботов-профи
        for (let i = 0; i < 9; i++) ws.send(Buffer.concat([u32(13), u32(2)]));
        setTimeout(() => {
          ws.send(Buffer.concat([u32(5), Buffer.from([1])])); // ready
          ws.send(u32(6)); // start
          c.phase = "playing";
        }, 700);
      } else if (tag === 6) {
        c.snapCount += 1;
      }
    };
    ws.onerror = (e) => reject(new Error(name + ": " + (e.message || "ws error")));
    resolve(c);
  });
}

function serverMetrics(pid) {
  // CPU% и RAM по PID через PowerShell
  return new Promise((resolve) => {
        exec(
      `powershell -NoProfile -Command "$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($p){ '{0} {1}' -f $p.CPU, $p.WorkingSet64 }"`,
      (err, stdout) => {
        const parts = String(stdout).trim().split(/\s+/);
        resolve({ cpuSecs: parseFloat(parts[0]) || 0, ramBytes: parseInt(parts[1]) || 0 });
      },
    );
  });
}

function pidFromLog() {
  return new Promise((resolve) => {
        exec(`powershell -NoProfile -Command "(Get-Process popproto-server -ErrorAction SilentlyContinue | Select-Object -First 1).Id"`,
      (err, stdout) => resolve(parseInt(String(stdout).trim()) || 0));
  });
}

(async () => {
  const pid = await pidFromLog();
  const before = await serverMetrics(pid);
  const t0 = Date.now();
  const clients = [];
  let failed = 0;
  const joined = (c) => c.phase !== "hello";
  for (let i = 0; i < ROOMS; i++) {
    let ok = false;
    for (let attempt = 0; attempt < 3 && !ok; attempt++) {
      const c = await client("H" + i + "-" + Math.floor(Math.random() * 90000 + 10000));
      await new Promise((r) => setTimeout(r, 1200)); // ждём Welcome
      if (joined(c)) { ok = true; clients.push(c); }
      else {
        try { c.ws.close(); } catch {}
        console.log(`  retry H${i} (attempt ${attempt + 1}, close=${c.closedCode ?? "?"})`);
        await new Promise((r) => setTimeout(r, 400));
      }
    }
    if (!ok) failed += 1;
  }
  console.log(`подключено ${clients.length}/${ROOMS} комнат (неуспешно: ${failed})`);

  // замер потока
  const b0 = clients.reduce((a, c) => a + c.bytes, 0);
  const m0 = clients.reduce((a, c) => a + c.msgs, 0);
  await new Promise((r) => setTimeout(r, SECONDS * 1000));
  const b1 = clients.reduce((a, c) => a + c.bytes, 0);
  const m1 = clients.reduce((a, c) => a + c.msgs, 0);
  const snaps = clients.reduce((a, c) => a + c.snapCount, 0);
  const after = await serverMetrics(pid);
  const wall = (Date.now() - t0) / 1000;

  const roomsRest = await fetch(`http://127.0.0.1:${port}/api/rooms`).then((r) => r.json()).catch(() => []);
  const playing = roomsRest.filter((r) => r.in_progress).length;

  console.log(`--- замер ${SECONDS} с ---`);
  console.log(`комнат по REST: ${roomsRest.length}, идёт матч: ${playing}`);
  console.log(`трафик: ${((b1 - b0) / SECONDS / 1024).toFixed(1)} КиБ/с суммарно вниз (${clients.length} клиентов)`);
  console.log(`сообщений/с: ${((m1 - m0) / SECONDS).toFixed(0)} (снапшотов всего за прогон: ${snaps})`);
  console.log(`CPU сервера: ${(((after.cpuSecs - before.cpuSecs) / SECONDS) * 100).toFixed(1)} % одного ядра`);
  console.log(`RAM сервера: ${(after.ramBytes / 1048576).toFixed(0)} МиБ`);
  clients.forEach((c) => c.ws.close());
  process.exit(0);
})();

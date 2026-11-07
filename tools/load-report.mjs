#!/usr/bin/env node
// Нагрузочный прогон и отчёт (без k6): входы, комнаты, матчи, p50/p95.
// Запуск: node tools/load-report.mjs [ws://127.0.0.1:8080] [clients] [seconds]
// Результат: tools/load-report.json + таблица в stdout (для диплома).
const HOST = process.argv[2] || "ws://127.0.0.1:8080";
const CLIENTS = Number(process.argv[3] || 50);
const SECONDS = Number(process.argv[4] || 30);

function u32(v) { const b = Buffer.alloc(4); b.writeUInt32LE(v, 0); return b; }
function u16(v) { const b = Buffer.alloc(2); b.writeUInt16LE(v, 0); return b; }
function u64(v) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v), 0); return b; }
function f32(v) { const b = Buffer.alloc(4); b.writeFloatLE(v, 0); return b; }
function str(s) { const e = Buffer.from(s, "utf8"); return Buffer.concat([u64(e.length), e]); }
const RUN = Date.now().toString(36).slice(-4);
const hello = (n) => Buffer.concat([u32(0), u16(6), str(n), Buffer.from([0])]);
const create = (n) => Buffer.concat([u32(2), str(n), Buffer.from([0]), u32(0), Buffer.from([10, 1, 1]), f32(1), Buffer.from([0])]);
const ready = () => Buffer.concat([u32(5), Buffer.from([1])]);
const start = () => u32(6);
const paddle = (x) => Buffer.concat([u32(8), f32(x)]);
const action = () => u32(9);

const stats = { connects: 0, rooms: 0, matches: 0, snapshots: 0, errors: 0, connectMs: [], snapInterval: [] };

function client(i) {
  return new Promise((resolve) => {
    const t0 = performance.now();
    const ws = new WebSocket(HOST + "/ws");
    ws.binaryType = "arraybuffer";
    ws.binaryType = "arraybuffer";
    let lastSnap = 0;
    let started = false;
    let created = false;
    ws.onopen = () => {
      stats.connects++;
      stats.connectMs.push(performance.now() - t0);
      const h = hello(`L${RUN}-${i}`);
      ws.send(h);
    };
    ws.onclose = (e) => console.log("[tool] close", e.code, "clean=" + e.wasClean);
    ws.onmessage = (e) => {
      const d = new Uint8Array(e.data);
      const tag = d[0];
      if (tag === 1) {
        const len = d[8] | (d[9] << 8) | (d[10] << 16) | (d[11] << 24);
        console.log("[tool] ERROR:", new TextDecoder().decode(d.slice(16, 16 + len)));
      }
      if (tag === 0 && !created) { // Welcome -> создать комнату
        created = true;
        ws.send(create(`L${RUN}-${i}`));
        setTimeout(() => { ws.send(ready()); ws.send(start()); }, 400);
      }
      if (tag === 4) { // RoomState
        if (!started) {
          stats.rooms++;
          ws.send(ready());
          setTimeout(() => { ws.send(start()); started = true; }, 300);
        }
      } else if (tag === 6) { // Snapshot
        stats.snapshots++;
        if (lastSnap) stats.snapInterval.push(performance.now() - lastSnap);
        lastSnap = performance.now();
        if (started && Math.random() < 0.05) ws.send(action());
        if (Math.random() < 0.4) ws.send(paddle(Math.random() * 800));
      } else if (tag === 8) stats.matches++;
    };
    ws.onerror = () => stats.errors++;
    ws.onclose = () => resolve();
    setTimeout(() => { try { ws.close(); } catch {} resolve(); }, SECONDS * 1000);
  });
}

const t0 = performance.now();
await Promise.all(Array.from({ length: CLIENTS }, (_, i) => client(i)));
const wall = (performance.now() - t0) / 1000;
const pct = (arr, p) => { if (!arr.length) return 0; const a = [...arr].sort((x, y) => x - y); return Math.round(a[Math.floor(a.length * p)]); };
const report = {
  host: HOST, clients: CLIENTS, seconds: SECONDS, wall: Math.round(wall),
  connects: stats.connects, rooms: stats.rooms, matches: stats.matches,
  snapshots: stats.snapshots, snapPerSec: Math.round(stats.snapshots / wall),
  errors: stats.errors,
  connect_p50: pct(stats.connectMs, 0.5), connect_p95: pct(stats.connectMs, 0.95),
  snapGap_p50: pct(stats.snapInterval, 0.5), snapGap_p95: pct(stats.snapInterval, 0.95),
};
console.table(report);
const fs = await import("node:fs");
fs.writeFileSync("tools/load-report.json", JSON.stringify(report, null, 2));
console.log("отчёт: tools/load-report.json");

// Generates a realistic 24h demo history and a high-noise evidence sample.
// Usage: node scripts/seed-demo.mjs [serverUrl] [deviceToken] [deviceId]
import fs from "node:fs";
import path from "node:path";

const serverUrl = (process.argv[2] || "http://127.0.0.1:8080").replace(/\/$/, "");
const deviceToken = process.argv[3] || "";
const deviceId = process.argv[4] || "";
const threshold = Number(process.env.THRESHOLD_DB || 65);
const now = new Date();
const root = path.resolve(import.meta.dirname, "..");
const dataDir = path.join(root, "data", "readings");
fs.mkdirSync(dataDir, { recursive: true });

let seed = 0x5eed1234;
function random() {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  return seed / 0xffffffff;
}

function baseLevel(hour) {
  if (hour >= 9 && hour < 12) return 54;
  if (hour >= 12 && hour < 14) return 49;
  if (hour >= 14 && hour < 18) return 57;
  if (hour >= 18 && hour < 20) return 50;
  return 36;
}

const rowsByDate = new Map();
const start = new Date(now.getTime() - 24 * 60 * 60 * 1000);
for (let time = start.getTime(); time <= now.getTime(); time += 30_000) {
  const date = new Date(time);
  const hour = date.getHours();
  const base = baseLevel(hour);
  const wave = Math.sin((time / 3_600_000) * 1.9) * 4.2;
  const noise = (random() - 0.5) * 9;
  const spike = random() > 0.988 ? 12 + random() * 14 : 0;
  const avg = Math.max(31, Math.min(96, base + wave + noise + spike));
  const peak = avg + 5 + random() * 9;
  const min = Math.max(26, avg - 7 - random() * 5);
  const row = {
    deviceId,
    recordedAt: date.toISOString(),
    avgDb: Number(avg.toFixed(1)),
    peakDb: Number(peak.toFixed(1)),
    minDb: Number(min.toFixed(1)),
    overThreshold: avg >= threshold,
    durationSecs: 30,
    sampleCount: 300,
  };
  const key = date.toISOString().slice(0, 10);
  const rows = rowsByDate.get(key) || [];
  rows.push(row);
  rowsByDate.set(key, rows);
}

for (const [date, rows] of rowsByDate) {
  const file = path.join(dataDir, `${date}.jsonl`);
  fs.writeFileSync(file, rows.map((row) => JSON.stringify(row)).join("\n") + "\n");
  console.log(`seeded ${rows.length} rows -> ${file}`);
}

async function postJson(pathname, body, headers = {}) {
  const response = await fetch(`${serverUrl}/api${pathname}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Device-Token": deviceToken,
      ...headers,
    },
    body: JSON.stringify(body),
  });
  return { status: response.status, body: await response.json().catch(() => null) };
}

const heartbeat = await postJson("/device/heartbeat", {
  deviceId,
  monitoring: true,
  appVersion: "0.1.0-demo",
});
console.log("heartbeat:", heartbeat);

// Build a 3-second 16 kHz WAV with a soft alarm-like tone for email evidence.
const sampleRate = 16000;
const seconds = 3;
const samples = sampleRate * seconds;
const dataSize = samples * 2;
const wav = Buffer.alloc(44 + dataSize);
wav.write("RIFF", 0);
wav.writeUInt32LE(36 + dataSize, 4);
wav.write("WAVE", 8);
wav.write("fmt ", 12);
wav.writeUInt32LE(16, 16);
wav.writeUInt16LE(1, 20);
wav.writeUInt16LE(1, 22);
wav.writeUInt32LE(sampleRate, 24);
wav.writeUInt32LE(sampleRate * 2, 28);
wav.writeUInt16LE(2, 32);
wav.writeUInt16LE(16, 34);
wav.write("data", 36);
wav.writeUInt32LE(dataSize, 40);
for (let index = 0; index < samples; index += 1) {
  const t = index / sampleRate;
  const value =
    Math.sin(2 * Math.PI * 440 * t) * 0.18 +
    Math.sin(2 * Math.PI * 880 * t) * 0.05 +
    (random() - 0.5) * 0.03;
  wav.writeInt16LE(Math.round(value * 32767), 44 + index * 2);
}

const currentAvg = 71.6 + random() * 4;
const telemetry = await postJson("/device/telemetry", {
  deviceId,
  avgDb: Number(currentAvg.toFixed(1)),
  peakDb: Number((currentAvg + 9).toFixed(1)),
  minDb: 52.4,
  durationSecs: 30,
  sampleCount: 300,
  recordedAt: new Date().toISOString(),
  audioBase64: wav.toString("base64"),
});
console.log("telemetry:", telemetry);
console.log(`dashboard: ${serverUrl}`);

// End-to-end tests against an isolated server and disposable data directory.
// Build first: cargo build --locked --manifest-path server/Cargo.toml
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, readFile, writeFile, mkdir, rename, utimes, appendFile, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(import.meta.dirname, '..');
const binary = process.env.SOUNDGUARD_SERVER || join(root, 'server/target/debug', process.platform === 'win32' ? 'soundguard-server.exe' : 'soundguard-server');
const data = await mkdtemp(join(tmpdir(), 'soundguard-test-'));
const port = await new Promise((resolvePort, reject) => {
  const server = createServer(); server.on('error', reject);
  server.listen(0, '127.0.0.1', () => { const port = server.address().port; server.close(() => resolvePort(port)); });
});
const base = `http://127.0.0.1:${port}`;
const sockets = [];
let child, log = '', checks = 0;
const env = { ...process.env, DATA_DIR: data, WEB_DIST: join(root, 'web/dist'), BIND_ADDR: '127.0.0.1', PORT: String(port),
  ADMIN_PASSWORD: 'Integration-pass-123', JWT_SECRET: 'test-only-secret-123456789012345678901234567890',
  DEVICE_ID: 'initial-device', DEVICE_TOKEN: 'initial-device-token-test-only', STOP_PIN: 'test-stop-password',
  SMTP_HOST: '', TLS_CERT: '', TLS_KEY: '', MEDIA_KEY_FILE: join(data, 'media-key'), CORS_ORIGINS: '',
  ALERT_TO: '', REPORT_TO: '', ADMIN_EMAIL: '', DAILY_REPORT_ENABLED: 'false', THRESHOLD_DB: '65', ALERT_COOLDOWN_SECS: '30',
  OFFLINE_TIMEOUT_SECS: '20', RETENTION_DAYS: '30' };
async function start() {
  child = spawn(binary, { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', b => { log += b; }); child.stderr.on('data', b => { log += b; });
  let spawnError; child.on('error', e => { spawnError = e; });
  for (let i = 0; i < 200; i++) {
    if (spawnError) throw spawnError;
    if (child.exitCode !== null) throw new Error(`Server exited: ${log}`);
    try { if ((await fetch(`${base}/api/health`)).ok) return; } catch {}
    await delay(50);
  }
  throw new Error(`Server did not start: ${log}`);
}
async function stop() {
  if (!child || child.exitCode !== null) return;
  for (const ws of sockets) ws.close();
  const stopping = child;
  const exited = new Promise(r => stopping.once('exit', r));
  const timer = setTimeout(() => stopping.kill('SIGKILL'), 6000);
  stopping.kill(process.platform === 'win32' ? 'SIGINT' : 'SIGTERM');
  await exited; clearTimeout(timer);
}
async function req(path, { token, deviceToken, body, method, status = 200 } = {}) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (deviceToken) headers['X-Device-Token'] = deviceToken;
  const response = await fetch(`${base}/api${path}`, { method: method || (body ? 'POST' : 'GET'), headers,
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(10000) });
  const text = await response.text();
  assert.equal(response.status, status, `${path}: ${text}`);
  checks++;
  let bodyResult; try { bodyResult = text ? JSON.parse(text) : null; } catch { bodyResult = { error: text }; }
  return { body: bodyResult, response };
}
async function openSocket(path) {
  const ws = new WebSocket(`${base.replace('http:', 'ws:')}/api${path}`);
  sockets.push(ws); ws.messages = []; ws.closed = false;
  ws.addEventListener('message', event => ws.messages.push(JSON.parse(event.data)));
  ws.addEventListener('close', () => { ws.closed = true; });
  await new Promise((resolveOpen, reject) => { ws.addEventListener('open', resolveOpen, { once: true }); ws.addEventListener('error', reject, { once: true }); });
  ws.wait = async predicate => {
    for (let i = 0; i < 80; i++) { const found = ws.messages.find(predicate); if (found) { checks++; return found; } await delay(25); }
    throw new Error(`Missing WS message: ${JSON.stringify(ws.messages)}`);
  };
  return ws;
}
function wav() {
  const b = Buffer.alloc(44 + 48000 * 2); b.write('RIFF'); b.writeUInt32LE(b.length - 8, 4); b.write('WAVE', 8);
  b.write('fmt ', 12); b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(48000, 24); b.writeUInt32LE(96000, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34);
  b.write('data', 36); b.writeUInt32LE(b.length - 44, 40); return b;
}
try {
  await start();
  const duplicateProcess = spawn(binary, { cwd: root, env, stdio: 'ignore' });
  assert.notEqual(await new Promise(r => duplicateProcess.on('exit', r)), 0); checks++;
  const login = async (username, password, status = 200) => (await req('/auth/login', { body: { username, password }, status })).body;
  const admin = (await login('admin', env.ADMIN_PASSWORD)).token;
  const auth = { token: admin };
  const health = await req('/health'); assert.equal(health.response.headers.get('cache-control'), 'no-store');
  await req('/auth/login', { body: { username: 'admin', password: 'x'.repeat(5000) }, status: 413 });
  await req('/devices', { status: 401 });
  await req('/devices', { token: 'invalid', status: 401 });
  const deviceA = (await req('/devices', { ...auth, body: { name: 'Room A', deviceId: 'room-a' } })).body;
  const deviceB = (await req('/devices', { ...auth, body: { name: 'Room B', deviceId: 'room-b' } })).body;
  const concurrent = await Promise.all([0, 1].map(async () => {
    const response = await fetch(`${base}/api/devices`, { method: 'POST', headers: { Authorization: `Bearer ${admin}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ name: 'Unique Room', deviceId: 'unique-room' }) });
    return response.status;
  })); assert.deepEqual(concurrent.sort(), [200, 409]); checks++;
  const retired = (await req('/users', { ...auth, body: { username: 'retired-admin', email: 'retired@example.com', password: 'Retired-pass-123', role: 'super_admin', deviceIds: [] } })).body;
  await req(`/users/${retired.id}`, { ...auth, method: 'PUT', body: { disabled: true } });
  await req(`/users/${retired.id}`, { ...auth, method: 'DELETE' });
  await req('/users', { ...auth, body: { username: 'invalid-email', email: 'bad@@example.com', password: 'Valid-pass-123', role: 'device_admin', deviceIds: [] }, status: 400 });
  const user = (await req('/users', { ...auth, body: { username: 'operator', email: 'operator@example.com', password: 'Operator-pass-123', role: 'device_admin', deviceIds: ['room-a'] } })).body;
  const operator = (await login('operator', 'Operator-pass-123')).token;
  const visible = (await req('/devices', { token: operator })).body.devices; assert.deepEqual(visible.map(d => d.id), ['room-a']);
  for (const path of ['/settings', '/users', '/overview?deviceId=room-b', '/readings?deviceId=room-b', '/alerts?deviceId=room-b', '/calls?deviceId=room-b']) await req(path, { token: operator, status: 403 });
  await req('/device/control', { token: operator, body: { deviceId: 'room-b', monitoring: false }, status: 403 });
  await req('/device/heartbeat', { deviceToken: deviceB.token, body: { deviceId: 'room-a', monitoring: true }, status: 401 });
  await req('/devices/room-a/settings', { ...auth, method: 'PUT', body: { thresholdDb: 55 } });
  const heartbeat = (await req('/device/heartbeat', { deviceToken: deviceA.token, body: { deviceId: 'room-a', monitoring: true } })).body;
  assert.equal(heartbeat.thresholdDb, 55);
  const live = { deviceId: 'room-a', currentDb: 61, averageDb: 60, peakDb: 65, capturing: true };
  await req('/device/live', { deviceToken: deviceA.token, body: live });
  const audio = wav();
  const telemetry = { deviceId: 'room-a', avgDb: 70, peakDb: 75, minDb: 50, durationSecs: 30, sampleCount: 1440000,
    recordedAt: new Date(Date.now() - 1200000).toISOString(), audioBase64: audio.toString('base64') };
  const first = (await req('/device/telemetry', { deviceToken: deviceA.token, body: telemetry })).body;
  assert.equal(first.overThreshold, true); assert.ok(first.alert?.evidenceFile);
  await req('/device/telemetry', { deviceToken: deviceA.token, body: telemetry });
  const readings = (await req('/readings?deviceId=room-a', auth)).body.readings;
  assert.equal(readings.length, 1); assert.equal(readings[0].recordedAt, telemetry.recordedAt);
  const evidence = await fetch(`${base}/api/alerts/${first.alert.id}/evidence`, { headers: { Authorization: `Bearer ${admin}` } });
  assert.equal(evidence.status, 200); assert.deepEqual(Buffer.from(await evidence.arrayBuffer()), audio); checks++;
  const stored = await readFile(join(data, 'evidence', first.alert.evidenceFile)); assert.equal(stored.subarray(0, 5).toString(), 'SGME1');
  await req('/device/telemetry', { deviceToken: deviceA.token, body: { ...telemetry, peakDb: 10 }, status: 400 });
  const call = { deviceId: 'room-a', mode: 'intercom', startedAt: new Date(Date.now()-1000).toISOString(), endedAt: new Date().toISOString() };
  await req('/calls', { ...auth, body: { ...call, audioBase64: Buffer.from('<script>bad</script>').toString('base64'), audioMime: 'text/html' }, status: 400 });
  await req('/calls', { ...auth, body: { ...call, audioBase64: 'invalid!', audioMime: 'audio/webm' }, status: 400 });
  const record = (await req('/calls', { ...auth, body: { ...call, audioBase64: audio.toString('base64'), audioMime: 'audio/wav' } })).body;
  const callAudio = await fetch(`${base}/api/device/calls/${record.id}/audio`, { headers: { 'X-Device-Token': deviceA.token } });
  assert.equal(callAudio.status, 200); assert.deepEqual(Buffer.from(await callAudio.arrayBuffer()), audio); checks++;
  await req(`/calls/${record.id}/audio`, { token: 'invalid', status: 401 });
  const adminWs = await openSocket(`/ws?token=${admin}`); await adminWs.wait(m => m.type === 'snapshot');
  const otherWs = await openSocket(`/ws?token=${admin}`); await otherWs.wait(m => m.type === 'snapshot');
  const operatorWs = await openSocket(`/ws?token=${operator}`); const snap = await operatorWs.wait(m => m.type === 'snapshot');
  assert.deepEqual(snap.data.devices.map(d => d.id), ['room-a']); assert.deepEqual(snap.data.settings.alertRecipients, []);
  const deviceWs = await openSocket(`/device/ws?deviceId=room-a&token=${deviceA.token}`); await deviceWs.wait(m => m.type === 'ready');
  adminWs.send(JSON.stringify('unexpected scalar')); deviceWs.send('null');
  adminWs.send(JSON.stringify({ type: 'signal', deviceId: 'room-a', callId: 'call-1', payload: { kind: 'offer', sdp: 'test' } }));
  await deviceWs.wait(m => m.callId === 'call-1');
  otherWs.send(JSON.stringify({ type: 'signal', deviceId: 'room-a', callId: 'call-2', payload: { kind: 'offer', sdp: 'test' } }));
  await otherWs.wait(m => m.type === 'signal-error' && m.callId === 'call-2');
  deviceWs.send(JSON.stringify({ type: 'signal', callId: 'call-1', payload: { kind: 'answer', sdp: 'answer' } }));
  await adminWs.wait(m => m.type === 'signal' && m.callId === 'call-1');
  await delay(100); assert.ok(!otherWs.messages.some(m => m.type === 'signal' && m.callId === 'call-1'));
  adminWs.close(); await deviceWs.wait(m => m.type === 'hangup' && m.callId === 'call-1');
  await req(`/users/${user.id}`, { ...auth, method: 'PUT', body: { deviceIds: [] } });
  await req('/device/live', { deviceToken: deviceA.token, body: live });
  for (let i=0; i<40 && !operatorWs.closed; i++) await delay(25);
  assert.ok(operatorWs.closed, 'assignment revocation closes the existing websocket'); checks++;
  await req('/devices', { token: operator, status: 401 });
  const rotated = (await req('/devices/room-a/token', { ...auth, body: {} })).body;
  await req('/device/heartbeat', { deviceToken: deviceA.token, body: { deviceId: 'room-a', monitoring: true }, status: 401 });
  await req('/device/heartbeat', { deviceToken: rotated.token, body: { deviceId: 'room-a', monitoring: true } });
  // Five failures for one account must not lock another account.
  for (let i=0; i<4; i++) await login('unknown-account', 'bad-password', 401);
  await login('unknown-account', 'bad-password', 429);
  await login('admin', env.ADMIN_PASSWORD);
  // Storage failure must return an error instead of acknowledging data loss.
  const todayFile = new Date().toISOString().slice(0,10) + '.jsonl';
  await rename(join(data, 'readings', todayFile), join(data, 'readings', todayFile + '.bak'));
  await mkdir(join(data, 'readings', todayFile));
  await req('/device/telemetry', { deviceToken: rotated.token, body: { ...telemetry, recordedAt: new Date().toISOString() }, status: 500 });
  await rm(join(data, 'readings', todayFile), { recursive: true });
  await rename(join(data, 'readings', todayFile + '.bak'), join(data, 'readings', todayFile));
  const statePath = join(data, 'state.json');
  await rename(statePath, statePath + '.backup');
  await mkdir(statePath);
  await req('/device/control', { ...auth, body: { deviceId: 'room-a', monitoring: false }, status: 500 });
  assert.equal((await req('/devices/room-a/status', auth)).body.device.desiredMonitoring, true);
  await rm(statePath, { recursive: true });
  await rename(statePath + '.backup', statePath);
  await stop();
  const dayPath = join(data, 'readings', telemetry.recordedAt.slice(0,10) + '.jsonl');
  await appendFile(dayPath, '{incomplete-row');
  const orphanPath = join(data, 'evidence', 'orphan.wav');
  await writeFile(orphanPath, stored);
  const past = new Date(Date.now() - 2 * 3600000);
  await utimes(orphanPath, past, past);
  const outboxPath = join(data, 'outbox', 'expired.txt');
  await writeFile(outboxPath, 'expired preview');
  const expired = new Date(Date.now() - 31 * 86400000);
  await utimes(outboxPath, expired, expired);
  await start();
  await req('/device/telemetry', { deviceToken: rotated.token, body: telemetry });
  assert.equal((await req('/readings?deviceId=room-a', auth)).body.readings.length, 1);
  assert.ok((await readFile(dayPath, 'utf8')).endsWith('\n'));
  await delay(100);
  await assert.rejects(access(orphanPath)); await assert.rejects(access(outboxPath)); checks += 2;
  await stop();
  // Corrupt state must never be overwritten by a newly bootstrapped admin.
  const original = await readFile(join(data, 'state.json'));
  await writeFile(join(data, 'state.json'), '{corrupt');
  const failed = spawn(binary, { cwd: root, env, stdio: 'ignore' });
  assert.notEqual(await new Promise(r => failed.on('exit', r)), 0);
  assert.equal(await readFile(join(data, 'state.json'), 'utf8'), '{corrupt'); checks++;
  await writeFile(join(data, 'state.json'), original);
  await rm(join(data, 'media-key'));
  const lostKey = spawn(binary, { cwd: root, env, stdio: 'ignore' });
  assert.notEqual(await new Promise(r => lostKey.on('exit', r)), 0); checks++;
  console.log(`PASS: ${checks} HTTP/WebSocket/storage checks`);
} catch (error) {
  console.error(error); console.error(log.slice(-6000)); process.exitCode = 1;
} finally { await stop(); await rm(data, { recursive: true, force: true }); }

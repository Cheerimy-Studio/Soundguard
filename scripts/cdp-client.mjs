// Dev-only helper for inspecting the running Tauri/WebView2 renderer.
// Requires the client to be launched with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
import fs from "node:fs";

const port = Number(process.env.CDP_PORT || 9222);
const args = process.argv.slice(2);
const mode = args[0] || "eval";
async function readExpression() {
  if (args[1]) return fs.readFileSync(args[1], "utf8");
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}
const expression =
  mode === "eval" ? await readExpression() : "document.title";

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const target = targets.find((item) => item.type === "page");
if (!target) throw new Error("No WebView2 page target found");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});
let nextId = 1;
const pending = new Map();
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
  }
});
function send(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

await send("Runtime.enable");
await send("Page.enable");
if (mode === "screenshot") {
  const result = await send("Page.captureScreenshot", { format: "png", fromSurface: true });
  const output = args[1] || "data/client-cdp.png";
  fs.writeFileSync(output, Buffer.from(result.data, "base64"));
  console.log(output);
} else {
  const result = await send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
    userGesture: true,
  });
  if (result.exceptionDetails) {
    console.error(JSON.stringify(result.exceptionDetails, null, 2));
    process.exitCode = 1;
  } else {
    console.log(JSON.stringify(result.result?.value ?? result.result, null, 2));
  }
}
socket.close();

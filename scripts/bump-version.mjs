// Bumps every shipped version location in one step.
// Usage: node scripts/bump-version.mjs [patch|minor|major|x.y.z]
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const mode = process.argv[2] || "patch";

function bump(version, kind) {
  const parts = version.split(".").map(Number);
  if (parts.length !== 3 || parts.some((n) => !Number.isInteger(n) || n < 0)) {
    throw new Error(`Invalid current version: ${version}`);
  }
  if (kind === "major") return `${parts[0] + 1}.0.0`;
  if (kind === "minor") return `${parts[0]}.${parts[1] + 1}.0`;
  if (kind === "patch") return `${parts[0]}.${parts[1]}.${parts[2] + 1}`;
  if (/^\d+\.\d+\.\d+$/.test(kind)) return kind;
  throw new Error("Usage: bump-version.mjs [patch|minor|major|x.y.z]");
}

function replaceOnce(file, pattern, replacement, label) {
  const full = path.join(root, file);
  const source = fs.readFileSync(full, "utf8");
  if (!pattern.test(source)) {
    throw new Error(`${label}: pattern not found in ${file}`);
  }
  fs.writeFileSync(full, source.replace(pattern, replacement));
}

const pkgPath = path.join(root, "package.json");
const current = JSON.parse(fs.readFileSync(pkgPath, "utf8")).version;
const next = bump(current, mode);

replaceOnce("package.json", /"version":\s*"[^"]+"/, `"version": "${next}"`, "root package.json");
replaceOnce("web/package.json", /"version":\s*"[^"]+"/, `"version": "${next}"`, "web package.json");
replaceOnce("web/package-lock.json", /"version":\s*"[^"]+"/, `"version": "${next}"`, "web package-lock version");
replaceOnce("web/package-lock.json", /("packages":\s*\{\s*"":\s*\{[^}]*?"version":\s*)"[^"]+"/s, `$1"${next}"`, "web package-lock root package");
replaceOnce("server/Cargo.toml", /(\[package\][\s\S]*?version\s*=\s*)"[^"]+"/, `$1"${next}"`, "server Cargo.toml");
replaceOnce("client-native/Cargo.toml", /(\[package\][\s\S]*?version\s*=\s*)"[^"]+"/, `$1"${next}"`, "client Cargo.toml");

for (const lock of ["server/Cargo.lock", "client-native/Cargo.lock"]) {
  const full = path.join(root, lock);
  const source = fs.readFileSync(full, "utf8");
  const packageName = lock.startsWith("server") ? "soundguard-server" : "soundguard-client-native";
  const pattern = new RegExp(`(\\[\\[package\\]\\]\\nname = "${packageName}"\\nversion = )"[^"]+"`);
  if (!pattern.test(source)) throw new Error(`local package not found in ${lock}`);
  fs.writeFileSync(full, source.replace(pattern, `$1"${next}"`));
}

// Android versionCode is monotonic; derive it from the version.
const android = path.join(root, "mobile/android/app/build.gradle");
const androidSource = fs.readFileSync(android, "utf8");
const parts = next.split(".").map(Number);
const code = parts[0] * 10000 + parts[1] * 100 + parts[2];
fs.writeFileSync(
  android,
  androidSource
    .replace(/versionCode\s+\d+/, `versionCode ${code}`)
    .replace(/versionName\s+"[^"]+"/, `versionName "${next}"`),
);

replaceOnce("deploy/windows/client.nsi", /!define VERSION "[^"]+"/, `!define VERSION "${next}"`, "client.nsi VERSION");
replaceOnce("deploy/windows/client.nsi", /VIProductVersion "[^"]+"/, `VIProductVersion "${next}.0"`, "client.nsi VIProductVersion");
replaceOnce("deploy/windows/client.nsi", /VIAddVersionKey "FileVersion" "[^"]+"/, `VIAddVersionKey "FileVersion" "${next}"`, "client.nsi FileVersion");
replaceOnce("deploy/windows/client.nsi", /VIAddVersionKey "ProductVersion" "[^"]+"/, `VIAddVersionKey "ProductVersion" "${next}"`, "client.nsi ProductVersion");
replaceOnce("deploy/windows/server.nsi", /!define VERSION "[^"]+"/, `!define VERSION "${next}"`, "server.nsi VERSION");

console.log(`版本号已更新：${current} → ${next}`);
console.log("请重新构建 Web、服务端、客户端、安装包与 APK。");

import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const outputs = [
  {
    source: path.join(root, "server", "target", "release", "soundguard-server.exe"),
    target: path.join(root, "dist", "soundguard-server.exe"),
  },
  {
    source: path.join(root, "client-native", "target", "release", "soundguard-client-native.exe"),
    target: path.join(root, "dist", "soundguard-client-native.exe"),
  },
];

fs.mkdirSync(path.join(root, "dist"), { recursive: true });
for (const output of outputs) {
  if (!fs.existsSync(output.source)) {
    throw new Error(`Missing release binary: ${output.source}. Run the release build first.`);
  }
  fs.copyFileSync(output.source, output.target);
  console.log(`collected ${path.relative(root, output.target)}`);
}

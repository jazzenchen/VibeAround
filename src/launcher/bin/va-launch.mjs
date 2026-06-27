#!/usr/bin/env node
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.platform === "win32" ? "va-launch.exe" : "va-launch";
const candidates = [
  process.env.VA_LAUNCH_BIN,
  join(here, exe),
  join(here, "..", "bin", exe),
  join(here, "..", "..", "target", "release", exe),
  join(here, "..", "..", "target", "debug", exe),
].filter(Boolean);

const binary = candidates.find((candidate) => existsSync(candidate));
if (!binary) {
  console.error("va-launch binary not found. Run `bun run --filter @va/launcher build` first.");
  process.exit(127);
}

const child = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (child.error) {
  console.error(child.error.message);
  process.exit(1);
}
process.exit(child.status ?? 1);

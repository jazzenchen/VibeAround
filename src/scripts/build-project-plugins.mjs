#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const SDK_PACKAGE_NAME = "@vibearound/plugin-channel-sdk";
const SDK_DIR_NAME = "va-plugin-channel-sdk";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const pluginsDir = resolve(scriptsDir, "..", "plugins");
const sdkDir = join(pluginsDir, SDK_DIR_NAME);

// Debug builds prefer source checkouts from this project directory. This
// script prepares only that first discovery tier; it never reads or writes
// the user plugin directory used as the runtime fallback.
function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function run(cwd, command, args) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed in ${cwd}`);
  }
}

function projectPluginDirs() {
  if (!existsSync(pluginsDir)) return [];

  return readdirSync(pluginsDir)
    .filter((entry) => {
      if (entry === SDK_DIR_NAME || entry.startsWith(".")) return false;
      const dir = join(pluginsDir, entry);
      if (!statSync(dir).isDirectory()) return false;
      return (
        existsSync(join(dir, "plugin.json")) &&
        existsSync(join(dir, "package.json"))
      );
    })
    .map((entry) => join(pluginsDir, entry))
    .filter((dir) => {
      const pkg = readJson(join(dir, "package.json"));
      return Boolean(pkg.dependencies?.[SDK_PACKAGE_NAME]);
    })
    .sort();
}

function main() {
  const hasLocalSdk = existsSync(join(sdkDir, "package.json"));
  if (hasLocalSdk) {
    console.log("[project-plugins] building local channel SDK");
    run(sdkDir, "npm", ["run", "build"]);
  } else {
    console.log(
      "[project-plugins] local channel SDK checkout not found; using declared npm dependencies",
    );
  }

  const pluginDirs = projectPluginDirs();
  console.log(`[project-plugins] building ${pluginDirs.length} project plugin(s)`);

  for (const pluginDir of pluginDirs) {
    const manifest = readJson(join(pluginDir, "plugin.json"));
    const installArgs = ["install", "--package-lock=false"];
    if (String(manifest.build ?? "").includes("--legacy-peer-deps")) {
      installArgs.splice(1, 0, "--legacy-peer-deps");
    }
    if (hasLocalSdk) {
      installArgs.push("--no-save", sdkDir);
    }

    console.log(
      `[project-plugins] ${manifest.id}: installing ${hasLocalSdk ? "local SDK" : "declared dependencies"}`,
    );
    run(pluginDir, "npm", installArgs);
    console.log(`[project-plugins] ${manifest.id}: building source`);
    run(pluginDir, "npm", ["run", "build"]);
  }
}

try {
  main();
} catch (error) {
  console.error(
    `[project-plugins] ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exit(1);
}

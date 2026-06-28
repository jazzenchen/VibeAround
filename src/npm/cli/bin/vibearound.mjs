#!/usr/bin/env node
import { existsSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const packageRoot = resolve(here, "..");
const rawArgs = process.argv.slice(2);
const args = rawArgs[0] === "--" ? rawArgs.slice(1) : rawArgs;

const tuiFlagsWithValues = new Set([
  "--auth-file",
  "--base-url",
  "--refresh-ms",
  "--token",
]);
const tuiInlineValuePrefixes = [...tuiFlagsWithValues].map((flag) => `${flag}=`);
const tuiBooleanFlags = new Set(["--once"]);

const route = routeArgs(args);
const env = packageEnv(route);
const binary = resolveNativeBinary(route.binary);

if (process.env.VIBEAROUND_NPM_CLI_DRY_RUN === "1") {
  console.log(
    JSON.stringify({
      binary: route.binary,
      args: route.args,
      nativeDir: nativeDirectory(),
      webDist: env.VIBEAROUND_WEB_DIST ?? null,
      vaLaunch: env.VIBEAROUND_VA_LAUNCH_BIN ?? null,
    }),
  );
  process.exit(0);
}

if (!binary) {
  failMissingBinary(route.binary);
}

const child = spawnSync(binary, route.args, {
  stdio: "inherit",
  env,
});

if (child.error) {
  console.error(`${basename(process.argv[1] ?? "vibearound")}: ${child.error.message}`);
  process.exit(1);
}

process.exit(child.status ?? 1);

function routeArgs(argv) {
  if (argv.length === 0) {
    return { binary: "va-tui", args: [] };
  }

  if (argv[0] === "tui" || argv[0] === "dashboard") {
    return { binary: "va-tui", args: argv.slice(1) };
  }

  if (argv.includes("--tui")) {
    return {
      binary: "va-tui",
      args: argv.filter((arg) => arg !== "--tui"),
    };
  }

  if (isTuiOnlyArgs(argv)) {
    return { binary: "va-tui", args: argv };
  }

  return { binary: "va-native", args: argv };
}

function isTuiOnlyArgs(argv) {
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];

    if (tuiBooleanFlags.has(arg)) {
      continue;
    }

    if (tuiInlineValuePrefixes.some((prefix) => arg.startsWith(prefix))) {
      continue;
    }

    if (tuiFlagsWithValues.has(arg)) {
      index += 1;
      continue;
    }

    return false;
  }

  return true;
}

function packageEnv(route) {
  const nativeDir = nativeDirectory();
  const webDist = join(packageRoot, "web", "dist");
  const vaLaunch = nativeBinaryPath("va-launch");
  const pathKey = pathEnvKey();
  const existingPath = process.env[pathKey] ?? "";
  const nextEnv = {
    ...process.env,
    [pathKey]: existingPath ? `${nativeDir}${delimiter()}${existingPath}` : nativeDir,
  };

  if (existsSync(vaLaunch)) {
    nextEnv.VIBEAROUND_VA_LAUNCH_BIN = vaLaunch;
  }

  if (route.binary === "va-native" && route.args[0] === "serve" && existsSync(webDist)) {
    const hasWebDistArg = route.args.some(
      (arg) => arg === "--web-dist" || arg.startsWith("--web-dist="),
    );
    if (!hasWebDistArg && !nextEnv.VIBEAROUND_WEB_DIST) {
      nextEnv.VIBEAROUND_WEB_DIST = webDist;
    }
  }

  return nextEnv;
}

function resolveNativeBinary(name) {
  const path = nativeBinaryPath(name);
  return existsSync(path) ? path : null;
}

function nativeBinaryPath(name) {
  return join(nativeDirectory(), `${name}${exeSuffix()}`);
}

function nativeDirectory() {
  return join(here, "native", `${process.platform}-${process.arch}`);
}

function exeSuffix() {
  return process.platform === "win32" ? ".exe" : "";
}

function delimiter() {
  return process.platform === "win32" ? ";" : ":";
}

function pathEnvKey() {
  return Object.keys(process.env).find((key) => key.toLowerCase() === "path") ?? "PATH";
}

function failMissingBinary(name) {
  console.error(
    [
      `vibearound: native binary '${name}' was not found for ${process.platform}-${process.arch}.`,
      "",
      "This package may have been published without binaries for your platform.",
      "If you are running from a source checkout, build the npm package first:",
      "  node src/npm/cli/scripts/prepare-package.mjs",
    ].join("\n"),
  );
  process.exit(127);
}

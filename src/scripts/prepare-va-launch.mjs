#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const srcRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const profile = valueFor("--profile") ?? "debug";
const desktopSidecars = [
  { binary: "va-launch", packageName: "va-launcher" },
  { binary: "va", packageName: "va-cli" },
  { binary: "va-tui", packageName: "va-tui" },
];

if (!args.includes("--desktop")) {
  fail("no package target selected; pass --desktop");
}

if (!["debug", "release"].includes(profile)) {
  fail(`unsupported --profile ${profile}; expected debug or release`);
}

const targetTriple = resolveTargetTriple();
const targetExtension = targetTriple.includes("windows") ? ".exe" : "";
const hostExtension = process.platform === "win32" ? ".exe" : "";

if (args.includes("--desktop")) {
  for (const sidecar of desktopSidecars) {
    prepareDesktopSidecar(sidecar, targetTriple, targetExtension);
  }
}

function prepareDesktopSidecar(sidecar, triple, extension) {
  const sourcePath = resolveBuiltBinary(sidecar, triple, extension);
  const binariesDir = join(srcRoot, "desktop", "binaries");
  const destination = join(binariesDir, `${sidecar.binary}-${triple}${extension}`);
  mkdirSync(binariesDir, { recursive: true });
  copyFileSync(sourcePath, destination);
  if (extension !== ".exe") {
    chmodSync(destination, 0o755);
  }
  console.log(`prepared ${destination}`);
}

function resolveBuiltBinary(sidecar, triple, extension) {
  const source = firstExisting([
    join(srcRoot, "target", triple, profile, `${sidecar.binary}${extension}`),
    join(srcRoot, "target", profile, `${sidecar.binary}${hostExtension}`),
    join(srcRoot, "target", profile, `${sidecar.binary}${extension}`),
  ]);
  if (source) {
    return source;
  }
  fail(
    `${sidecar.binary} binary not found for ${profile}; run cargo build ${
      profile === "release" ? "--release " : ""
    }-p ${sidecar.packageName} first`,
  );
}

function resolveTargetTriple() {
  const configured =
    process.env.TAURI_ENV_TARGET_TRIPLE ??
    process.env.TARGET ??
    process.env.CARGO_BUILD_TARGET;
  if (configured) {
    return configured;
  }

  const rustc = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  if (rustc.status !== 0) {
    fail("could not resolve target triple with rustc -vV");
  }

  const hostLine = rustc.stdout
    .split("\n")
    .find((line) => line.startsWith("host: "));
  if (!hostLine) {
    fail("rustc -vV did not report a host triple");
  }
  return hostLine.slice("host: ".length).trim();
}

function firstExisting(paths) {
  return paths.find((path) => existsSync(path));
}

function valueFor(flag) {
  const index = args.indexOf(flag);
  if (index === -1) {
    return null;
  }
  const value = args[index + 1];
  if (!value || value.startsWith("--")) {
    fail(`${flag} requires a value`);
  }
  return value;
}

function fail(message) {
  console.error(`prepare-va-launch: ${message}`);
  process.exit(1);
}

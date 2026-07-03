#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const srcRoot = resolve(packageRoot, "..", "..");
const repoRoot = resolve(srcRoot, "..");
const args = process.argv.slice(2);
const profile = valueFor("--profile") ?? "release";
const skipBuild = args.includes("--skip-build");
const skipWeb = args.includes("--skip-web");
const skipWebBuild = args.includes("--skip-web-build");
const platform = valueFor("--platform") ?? process.platform;
const arch = valueFor("--arch") ?? process.arch;
const signDarwin = args.includes("--sign") || process.env.VIBEAROUND_NPM_SIGN === "1";
const signingIdentity =
  valueFor("--signing-identity") ??
  process.env.VIBEAROUND_CODESIGN_IDENTITY ??
  process.env.APPLE_SIGNING_IDENTITY;

if (!["debug", "release"].includes(profile)) {
  fail(`unsupported --profile ${profile}; expected debug or release`);
}

const targetDir = join(srcRoot, "target", profile);
const nativeDir = join(packageRoot, "bin", "native", `${platform}-${arch}`);
const extension = platform === "win32" ? ".exe" : "";
const cargoProfileArgs = profile === "release" ? ["--release"] : [];

if (!skipBuild) {
  run("cargo", [
    "build",
    "--manifest-path",
    join(srcRoot, "Cargo.toml"),
    ...cargoProfileArgs,
    "-p",
    "va-cli",
    "-p",
    "va-tui",
    "-p",
    "va-launcher",
    "-p",
    "server",
  ]);
}

rmSync(nativeDir, { recursive: true, force: true });
mkdirSync(nativeDir, { recursive: true });

copyBinary("va", "va-native");
copyBinary("va-tui", "va-tui");
copyBinary("va-launch", "va-launch");
copyBinary("vibearound-server", "vibearound-server");

if (platform === "darwin" && signDarwin) {
  signDarwinBinaries();
}

if (!skipWeb) {
  const webDist = join(srcRoot, "web", "dist");
  if (!skipWebBuild && !existsSync(join(webDist, "index.html"))) {
    run("bun", ["run", "--cwd", srcRoot, "web:build"]);
  }

  if (existsSync(join(webDist, "index.html"))) {
    const packageWebDist = join(packageRoot, "web", "dist");
    rmSync(packageWebDist, { recursive: true, force: true });
    mkdirSync(dirnamePath(packageWebDist), { recursive: true });
    cpSync(webDist, packageWebDist, { recursive: true });
  } else {
    fail(`web dashboard dist not found at ${webDist}; run bun run --cwd src web:build`);
  }
}

const licenseSource = join(repoRoot, "LICENSE");
if (existsSync(licenseSource)) {
  copyFileSync(licenseSource, join(packageRoot, "LICENSE"));
}

console.log(`prepared vibearound npm package for ${platform}-${arch} (${profile})`);

function copyBinary(sourceName, destinationName) {
  const source = join(targetDir, `${sourceName}${extension}`);
  if (!existsSync(source)) {
    fail(`${sourceName}${extension} not found at ${source}`);
  }
  const destination = join(nativeDir, `${destinationName}${extension}`);
  copyFileSync(source, destination);
  if (platform !== "win32") {
    chmodSync(destination, 0o755);
  }
}

function signDarwinBinaries() {
  if (!signingIdentity) {
    fail(
      "--sign requires APPLE_SIGNING_IDENTITY, VIBEAROUND_CODESIGN_IDENTITY, or --signing-identity",
    );
  }

  for (const binary of [
    "va-native",
    "va-tui",
    "va-launch",
    "vibearound-server",
  ]) {
    const path = join(nativeDir, binary);
    runQuiet("codesign", [
      "--force",
      "--options",
      "runtime",
      "--timestamp",
      "--sign",
      signingIdentity,
      path,
    ]);
    runQuiet("codesign", ["--verify", "--verbose=2", path]);
  }
  console.log("signed Darwin native binaries");
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

function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (result.error) {
    fail(result.error.message);
  }
  if (result.status !== 0) {
    fail(`${command} ${commandArgs.join(" ")} exited with ${result.status}`);
  }
}

function runQuiet(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (result.error) {
    fail(result.error.message);
  }
  if (result.status !== 0) {
    fail(`${command} exited with ${result.status}`);
  }
}

function dirnamePath(path) {
  return dirname(path);
}

function fail(message) {
  console.error(`prepare-package: ${message}`);
  process.exit(1);
}

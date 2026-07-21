#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SDK_PACKAGE_NAME = "@vibearound/plugin-channel-sdk";
const scriptsDir = dirname(fileURLToPath(import.meta.url));
const catalogPath = resolve(scriptsDir, "..", "resources", "plugins.json");

function fail(message) {
  throw new Error(`[channel-catalog] ${message}`);
}

function repositoryParts(github) {
  const url = new URL(github);
  const parts = url.pathname.replace(/^\//, "").replace(/\.git$/, "").split("/");
  if (url.hostname !== "github.com" || parts.length !== 2 || parts.some((part) => !part)) {
    fail(`unsupported repository URL: ${github}`);
  }
  return parts;
}

function mainRevision(github) {
  const output = execFileSync("git", ["ls-remote", github, "refs/heads/main"], {
    encoding: "utf8",
  }).trim();
  const revision = output.split(/\s+/)[0];
  if (!/^[0-9a-f]{40}$/.test(revision ?? "")) {
    fail(`cannot resolve main for ${github}`);
  }
  return revision;
}

async function fetchJson(owner, repo, revision, path) {
  const url = `https://raw.githubusercontent.com/${owner}/${repo}/${revision}/${path}`;
  const response = await fetch(url, { headers: { "user-agent": "VibeAround-release-check" } });
  if (!response.ok) fail(`${url} returned HTTP ${response.status}`);
  return response.json();
}

async function main() {
  const catalog = JSON.parse(readFileSync(catalogPath, "utf8"));
  const channels = catalog.filter((plugin) => plugin.kind === "channel");
  if (channels.length === 0) fail("catalog has no channel plugins");

  const pluginVersions = new Set();
  const sdkVersions = new Set();

  for (const plugin of channels) {
    const remoteRevision = mainRevision(plugin.github);
    if (plugin.revision !== remoteRevision) {
      fail(`${plugin.id} pins ${plugin.revision}, but main is ${remoteRevision}`);
    }

    const [owner, repo] = repositoryParts(plugin.github);
    const [pkg, manifest, lock] = await Promise.all([
      fetchJson(owner, repo, plugin.revision, "package.json"),
      fetchJson(owner, repo, plugin.revision, "plugin.json"),
      fetchJson(owner, repo, plugin.revision, "package-lock.json"),
    ]);
    const lockedRoot = lock.packages?.[""];
    const lockedSdk = lock.packages?.[`node_modules/${SDK_PACKAGE_NAME}`]?.version;
    const declaredSdk = pkg.dependencies?.[SDK_PACKAGE_NAME];

    if (manifest.id !== plugin.id) fail(`${plugin.id} manifest id is ${manifest.id}`);
    if (pkg.version !== manifest.version || pkg.version !== lockedRoot?.version) {
      fail(`${plugin.id} package, manifest, and lockfile versions are not aligned`);
    }
    if (!lockedSdk || declaredSdk !== `^${lockedSdk}`) {
      fail(`${plugin.id} SDK dependency and lockfile are not aligned`);
    }

    pluginVersions.add(pkg.version);
    sdkVersions.add(lockedSdk);
    console.log(`[channel-catalog] ${plugin.id} ${pkg.version} @ ${plugin.revision.slice(0, 7)}`);
  }

  if (pluginVersions.size !== 1) fail("channel plugin versions are not aligned");
  if (sdkVersions.size !== 1) fail("channel SDK versions are not aligned");

  const [pluginVersion] = pluginVersions;
  const [sdkVersion] = sdkVersions;
  const npmResponse = await fetch(
    "https://registry.npmjs.org/@vibearound%2fplugin-channel-sdk/latest",
  );
  if (!npmResponse.ok) fail(`npm registry returned HTTP ${npmResponse.status}`);
  const publishedSdk = await npmResponse.json();
  if (publishedSdk.version !== sdkVersion) {
    fail(`plugins require SDK ${sdkVersion}, but npm latest is ${publishedSdk.version}`);
  }

  console.log(
    `[channel-catalog] verified ${channels.length} plugins at ${pluginVersion} with published SDK ${sdkVersion}`,
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});

#!/usr/bin/env node
// Rewrite every plugin's `@vibearound/plugin-channel-sdk` dependency to either
// the local source tree (dev) or the published npm version (release), then
// reinstall.
//
// Why not a Bun workspace?
//   We want each plugin's `package.json` to stay valid for standalone
//   `npm install` / `bun install` outside the repo — that's how the desktop
//   app's plugin install flow works at runtime (it copies a plugin dir into
//   `~/.vibearound/plugins/<id>` and runs `npm install && npm run build`
//   against the public registry). A `workspace:*` specifier would break that.
//
// Usage (from any cwd, but paths are resolved relative to this file):
//   node src/scripts/link-sdk.mjs --mode=local     # dev: file:../va-plugin-channel-sdk
//   node src/scripts/link-sdk.mjs --mode=release   # ship: ^<sdk version>
//
// Lives in `src/scripts/` (tracked by the main repo) rather than
// `src/plugins/scripts/` because `src/plugins/` is gitignored — the
// plugins themselves live in separate git repos, but this orchestration
// script must ship with the main VibeAround repo so `cargo tauri dev`
// and `cargo tauri build` can invoke it on any clone.
//
// Modes:
//   local    — point every plugin at `file:../va-plugin-channel-sdk` and run
//              `bun install` so hot edits to the SDK source are picked up on
//              the next plugin restart. Mutates tracked `package.json`
//              files; remember to `--mode=release` before committing.
//
//   release  — rewrite back to `^<version>` where <version> is read from
//              `va-plugin-channel-sdk/package.json`. This is the canonical committed
//              state; `build.sh` and Tauri's `beforeBuildCommand` call this
//              mode before building so shipped artifacts link against the
//              public SDK on the npm registry.
//
// Idempotent: running the same mode twice is a no-op. Safe to wire into
// `beforeBuildCommand` / `beforeDevCommand`.

import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const SDK_PACKAGE_NAME = '@vibearound/plugin-channel-sdk';
const SDK_DIR_NAME = 'va-plugin-channel-sdk';
const LOCAL_SPEC = 'file:../va-plugin-channel-sdk';

const __filename = fileURLToPath(import.meta.url);
const scriptsDir = dirname(__filename);
// scriptsDir = <repo>/src/scripts — plugins live at <repo>/src/plugins
const pluginsDir = resolve(scriptsDir, '..', 'plugins');

function parseArgs(argv) {
    const args = { mode: null, skipInstall: false };
    for (const arg of argv.slice(2)) {
        if (arg.startsWith('--mode=')) {
            args.mode = arg.slice('--mode='.length);
        } else if (arg === '--mode') {
            // handled by next iteration — simpler to require `--mode=xxx` form
            throw new Error('use --mode=local or --mode=release (with =)');
        } else if (arg === '--skip-install') {
            args.skipInstall = true;
        } else {
            throw new Error(`unknown argument: ${arg}`);
        }
    }
    if (args.mode !== 'local' && args.mode !== 'release') {
        throw new Error('missing or invalid --mode= (expected local|release)');
    }
    return args;
}

function readJson(path) {
    return JSON.parse(readFileSync(path, 'utf8'));
}

function writeJson(path, obj) {
    // Preserve trailing newline + 2-space indent to match existing style.
    writeFileSync(path, JSON.stringify(obj, null, 2) + '\n');
}

function listPluginDirs() {
    if (!existsSync(pluginsDir)) {
        return [];
    }
    return readdirSync(pluginsDir)
        .filter((entry) => {
            const full = join(pluginsDir, entry);
            if (entry === SDK_DIR_NAME) return false;
            if (entry === 'scripts') return false;
            if (entry.startsWith('.')) return false;
            if (!statSync(full).isDirectory()) return false;
            return existsSync(join(full, 'package.json'));
        })
        .sort();
}

function targetSpec(mode) {
    if (mode === 'local') return LOCAL_SPEC;
    const sdkPkg = readJson(sdkPackagePath());
    if (!sdkPkg.version) {
        throw new Error('va-plugin-channel-sdk/package.json is missing "version"');
    }
    return `^${sdkPkg.version}`;
}

function sdkPackagePath() {
    return join(pluginsDir, SDK_DIR_NAME, 'package.json');
}

/**
 * Decide whether a plugin is "managed" by this script.
 *
 * To prevent this script from accidentally bumping plugins that are
 * intentionally pinned to an older SDK (e.g. the "planned but not yet
 * ported" channel plugins tracked in separate repos), we only touch a
 * plugin if its *current* SDK spec is one of:
 *
 *   - `file:../va-plugin-channel-sdk`  — it's in local mode already
 *   - `^<current-sdk-version>` — it's in release mode already
 *
 * A plugin opts in by manually bumping its SDK pin to the current
 * published version once; from then on the script alternates it
 * between the two modes automatically. Plugins pinned to `^0.1.2`
 * (or any other spec) are left untouched.
 */
function isManaged(currentSpec, releaseSpec) {
    return currentSpec === LOCAL_SPEC || currentSpec === releaseSpec;
}

function rewritePluginDep(pluginDir, targetSpec, releaseSpec) {
    const pkgPath = join(pluginDir, 'package.json');
    const pkg = readJson(pkgPath);
    const deps = pkg.dependencies ?? {};
    if (!(SDK_PACKAGE_NAME in deps)) {
        return { changed: false, reason: 'no channel-sdk dep' };
    }
    const current = deps[SDK_PACKAGE_NAME];
    if (!isManaged(current, releaseSpec)) {
        return { changed: false, reason: `not managed (current=${current})` };
    }
    if (current === targetSpec) {
        return { changed: false, reason: 'already at target spec' };
    }
    deps[SDK_PACKAGE_NAME] = targetSpec;
    pkg.dependencies = deps;
    writeJson(pkgPath, pkg);
    return { changed: true, previous: current };
}

function runInstall(pluginDir) {
    // Bun is the canonical install tool for this workspace. Fall back to npm
    // only if bun isn't on PATH — some CI environments might not have it.
    const tool = spawnSync('bun', ['--version'], { stdio: 'ignore' }).status === 0
        ? 'bun'
        : 'npm';
    const result = spawnSync(tool, ['install'], {
        cwd: pluginDir,
        stdio: 'inherit',
    });
    if (result.status !== 0) {
        throw new Error(`${tool} install failed in ${pluginDir}`);
    }
}

function main() {
    const args = parseArgs(process.argv);
    const sdkPkgPath = sdkPackagePath();
    if (!existsSync(sdkPkgPath)) {
        console.log(`[link-sdk] ${sdkPkgPath} not found`);
        console.log('[link-sdk] local plugin SDK checkout is optional; skipping dependency rewrite');
        return;
    }

    const releaseSpec = targetSpec('release');
    const spec = args.mode === 'release' ? releaseSpec : LOCAL_SPEC;
    const plugins = listPluginDirs();

    console.log(`[link-sdk] mode=${args.mode} spec=${spec}`);
    console.log(`[link-sdk] plugins dir: ${pluginsDir}`);
    console.log(`[link-sdk] ${plugins.length} plugin(s) discovered`);

    const changedDirs = [];
    for (const name of plugins) {
        const pluginDir = join(pluginsDir, name);
        const result = rewritePluginDep(pluginDir, spec, releaseSpec);
        if (result.changed) {
            console.log(`  ${name}: ${result.previous} → ${spec}`);
            changedDirs.push(pluginDir);
        } else {
            console.log(`  ${name}: skipped (${result.reason})`);
        }
    }

    if (changedDirs.length === 0) {
        console.log('[link-sdk] nothing to do');
        return;
    }

    if (args.skipInstall) {
        console.log('[link-sdk] --skip-install set, leaving node_modules as-is');
        return;
    }

    console.log(`[link-sdk] reinstalling dependencies in ${changedDirs.length} plugin(s)`);
    for (const dir of changedDirs) {
        runInstall(dir);
    }
    console.log('[link-sdk] done');
}

try {
    main();
} catch (err) {
    console.error(`[link-sdk] error: ${err.message}`);
    process.exit(1);
}

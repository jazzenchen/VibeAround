#!/usr/bin/env bun
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const srcRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const rawArgs = Bun.argv.slice(2);
const args = rawArgs[0] === "--" ? rawArgs.slice(1) : rawArgs;

const tuiFlagsWithValues = new Set([
  "--auth-file",
  "--base-url",
  "--refresh-ms",
  "--token",
]);
const tuiInlineValuePrefixes = [...tuiFlagsWithValues].map((flag) => `${flag}=`);
const tuiBooleanFlags = new Set(["--once"]);

function routeArgs(argv) {
  if (argv.length === 0) {
    return { packageName: "va-tui", args: [] };
  }

  if (argv[0] === "tui" || argv[0] === "dashboard") {
    return { packageName: "va-tui", args: argv.slice(1) };
  }

  if (argv.includes("--tui")) {
    return {
      packageName: "va-tui",
      args: argv.filter((arg) => arg !== "--tui"),
    };
  }

  if (isTuiOnlyArgs(argv)) {
    return { packageName: "va-tui", args: argv };
  }

  return { packageName: "va-cli", args: argv };
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

const route = routeArgs(args);
const command = ["cargo", "run", "-p", route.packageName, "--", ...route.args];

if (process.env.VA_LAUNCHER_DRY_RUN === "1") {
  console.log(JSON.stringify({ packageName: route.packageName, args: route.args, command }));
  process.exit(0);
}

const child = Bun.spawn(command, {
  cwd: srcRoot,
  stdin: "inherit",
  stdout: "inherit",
  stderr: "inherit",
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    child.kill(signal);
  });
}

const exitCode = await child.exited;
process.exit(exitCode);

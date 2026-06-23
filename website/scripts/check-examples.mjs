#!/usr/bin/env node
// Anti-drift guarantee: every .li file embedded by the site must actually
// compile and run. This script discovers them, runs each through the Lira
// CLI, and exits non-zero if any fail. Wired into `npm run build`.
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const websiteDir = resolve(here, "..");
const repoRoot = resolve(websiteDir, "..");
const srcDir = join(websiteDir, "src");
const snippetsDir = join(srcDir, "snippets");

// 1. The Lira CLI (built in release mode).
const bin = join(repoRoot, "target", "release", "lira");
if (!existsSync(bin)) {
  console.error(
    `\n  Lira CLI not found at ${relative(repoRoot, bin)}\n` +
      `  Build it first:  cargo build --release -p lira\n`,
  );
  process.exit(1);
}

// 2. Walk src/ collecting every "examples/..." path referenced by the site.
function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

const referenced = new Set();

// Snippets: every .li under src/snippets is embedded by definition.
if (existsSync(snippetsDir)) {
  for (const f of readdirSync(snippetsDir)) {
    if (f.endsWith(".li")) referenced.add(join(snippetsDir, f));
  }
}

// References to repo-level .li files passed to <Code/>, either as a JSX prop
//   file="examples/result_propagation.li"   /   file={"stdlib/x.li"}
// or as an object field used to build the cards
//   { file: "examples/generic_enum.li" }
// Scoping to the `file` key keeps prose/comments mentioning paths from counting.
const refRe =
  /\bfile\s*[:=]\s*\{?\s*['"`]((?:examples|stdlib|website)\/[A-Za-z0-9_\-/]+\.li)['"`]/g;
for (const file of walk(srcDir)) {
  if (!/\.(astro|mjs|js|ts|md|mdx)$/.test(file)) continue;
  const text = readFileSync(file, "utf-8");
  let m;
  while ((m = refRe.exec(text)) !== null) {
    referenced.add(join(repoRoot, m[1]));
  }
}

const files = [...referenced].sort();
if (files.length === 0) {
  console.error("  No embedded .li files found — nothing to verify.");
  process.exit(1);
}

console.log(`\n  Verifying ${files.length} embedded Lira snippet(s)...\n`);

let failed = 0;
for (const file of files) {
  const label = relative(repoRoot, file);
  if (!existsSync(file)) {
    console.error(`  MISSING  ${label}`);
    failed++;
    continue;
  }
  try {
    execFileSync(bin, ["run", file], { stdio: "pipe" });
    console.log(`  ok       ${label}`);
  } catch (err) {
    failed++;
    console.error(`  FAILED   ${label}`);
    const out = (err.stdout?.toString() ?? "") + (err.stderr?.toString() ?? "");
    for (const line of out.trim().split("\n")) console.error(`           ${line}`);
  }
}

if (failed > 0) {
  console.error(`\n  ${failed} snippet(s) failed to run. Fix them before building.\n`);
  process.exit(1);
}
console.log(`\n  All ${files.length} snippet(s) compile and run.\n`);

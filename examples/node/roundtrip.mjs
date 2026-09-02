#!/usr/bin/env node
// Small CLI integration example for a UI-automation action prior.
// The domain adapter owns the screen/action vocabulary; lineprior only sees JSONL.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const input = join(root, "ui_automation.jsonl");
const binary = process.env.LINEPRIOR_BIN ?? "lineprior";
const directory = mkdtempSync(join(tmpdir(), "lineprior-node-"));
const first = join(directory, "prior-1.jsonl");
const second = join(directory, "prior-2.jsonl");

try {
  const run = (...args) => execFileSync(binary, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  run("build", input, "--out", first);
  run("build", input, "--out", second);
  if (readFileSync(first, "utf8") !== readFileSync(second, "utf8")) {
    throw new Error("repeated builds were not deterministic");
  }
  const output = run("query", first, "--state", "cart-empty", "--top-k", "1");
  const row = output.trim().split("\n").filter(Boolean).map(JSON.parse)[0];
  if (row?.action !== "click:add-to-cart") {
    throw new Error(`unexpected query result: ${JSON.stringify(row)}`);
  }
  console.log(JSON.stringify(row));
} finally {
  rmSync(directory, { recursive: true, force: true });
}

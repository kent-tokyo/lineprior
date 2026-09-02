import { chromium } from "playwright";
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";

const packageDir = resolve(process.argv[2] ?? "target/wasm-web");
const packageEntry = join(packageDir, "lineprior_wasm.js");
statSync(packageEntry);
const server = createServer((request, response) => {
  const relative = request.url?.startsWith("/pkg/")
    ? decodeURIComponent(request.url.slice("/pkg/".length))
    : "index.html";
  const file = join(packageDir, relative);
  try {
    const stat = statSync(file);
    if (!stat.isFile()) throw new Error("not a file");
    response.setHeader("Content-Type", file.endsWith(".wasm") ? "application/wasm" : "text/javascript");
    createReadStream(file).pipe(response);
  } catch {
    response.statusCode = 404;
    response.end("not found");
  }
});
await new Promise((resolveServer) => server.listen(0, "127.0.0.1", resolveServer));
const { port } = server.address();
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${port}/index.html`);
await page.evaluate(async () => {
  const module = await import("/pkg/lineprior_wasm.js");
  await module.default("/pkg/lineprior_wasm_bg.wasm");
  const result = JSON.parse(module.build_json(
    '{"sequence_id":"browser","step":0,"state":"screen","action":"click","outcome":"success"}\n',
    '{}',
  ));
  if (result.entries[0].state !== "screen") throw new Error("unexpected build state");
  const query = JSON.parse(module.query_json(JSON.stringify(result.entries[0]) + "\n", "screen", 1));
  if (query[0].action !== "click") throw new Error("unexpected query action");
});
await browser.close();
server.close();
console.log("WASM browser smoke: ok");

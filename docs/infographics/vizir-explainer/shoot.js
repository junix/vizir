#!/usr/bin/env node
// Minimal CDP screenshot driver for the static infographic page.
// Usage: node shoot.js <file-url> <outdir>
// Produces fixed-height full-width slices from y=0 (stitched later), plus
// sections.json recording per-panel geometry for 1:1 crops.
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");

const SHELL = process.env.HEADLESS_SHELL || process.env.HOME +
  "/Library/Caches/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell";
const WIDTH = 1200;
const SLICE = 3600; // CSS px per slice (7200 device px at dpr 2)

function connect(wsUrl) {
  return new Promise((res, rej) => {
    const ws = new WebSocket(wsUrl);
    ws.onerror = () => rej(new Error("ws error"));
    ws.onopen = () => res(ws);
  });
}

function cdpClient(ws) {
  let id = 0;
  const pending = new Map();
  const waiters = [];
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { res, rej } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? rej(new Error(msg.error.message)) : res(msg.result);
    } else if (msg.method) {
      for (let i = waiters.length - 1; i >= 0; i--) {
        if (waiters[i].method === msg.method &&
            (!waiters[i].sessionId || waiters[i].sessionId === msg.sessionId)) {
          const w = waiters.splice(i, 1)[0];
          w.res(msg.params);
        }
      }
    }
  };
  return {
    send(method, params = {}, sessionId) {
      const mid = ++id;
      return new Promise((res, rej) => {
        pending.set(mid, { res, rej });
        ws.send(JSON.stringify({ id: mid, method, params, sessionId }));
      });
    },
    once(method, sessionId) {
      return new Promise((res) => waiters.push({ method, res, sessionId }));
    },
  };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const url = process.argv[2];
  const outdir = process.argv[3];
  fs.mkdirSync(outdir, { recursive: true });
  const udd = fs.mkdtempSync("/tmp/shoot-profile-");
  const proc = spawn(SHELL, [
    "--remote-debugging-port=0",
    `--user-data-dir=${udd}`,
    "--no-first-run", "--no-default-browser-check",
    "--disable-gpu", "--hide-scrollbars", "--force-color-profile=srgb",
    "about:blank",
  ], { stdio: ["ignore", "ignore", "pipe"] });
  proc.stderr.on("data", () => {});
  const portFile = path.join(udd, "DevToolsActivePort");
  for (let i = 0; i < 100 && !fs.existsSync(portFile); i++) await sleep(100);
  const port = fs.readFileSync(portFile, "utf8").split("\n")[0];
  const ver = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  const ws = await connect(ver.webSocketDebuggerUrl);
  const c = cdpClient(ws);

  const { targetId } = await c.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await c.send("Target.attachToTarget", { targetId, flatten: true });
  await c.send("Page.enable", {}, sessionId);
  await c.send("Runtime.enable", {}, sessionId);

  const loaded = c.once("Page.loadEventFired", sessionId);
  await c.send("Page.navigate", { url }, sessionId);
  await loaded;
  await c.send("Runtime.evaluate", {
    expression: "document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))",
    awaitPromise: true,
  }, sessionId);

  const dims = await c.send("Runtime.evaluate", {
    expression: "JSON.stringify({w: document.documentElement.scrollWidth, h: document.documentElement.scrollHeight})",
    returnByValue: true,
  }, sessionId);
  const { w, h } = JSON.parse(dims.result.value);
  console.log(`page ${w}x${h}`);
  if (w !== WIDTH) {
    console.error(`FATAL: page width ${w} != ${WIDTH}`);
    process.exit(1);
  }

  const secs = await c.send("Runtime.evaluate", {
    expression: `JSON.stringify(Array.from(document.querySelectorAll('header,section.chapter,footer')).map(e => ({
      id: e.id || e.tagName.toLowerCase(), y: Math.round(e.getBoundingClientRect().top + window.scrollY),
      h: Math.round(e.getBoundingClientRect().height)})))`,
    returnByValue: true,
  }, sessionId);
  if (!secs.result.value) {
    console.error("sections eval failed:", JSON.stringify(secs).slice(0, 400));
    process.exit(1);
  }
  const sections = JSON.parse(secs.result.value);
  fs.writeFileSync(path.join(outdir, "sections.json"),
                   JSON.stringify({ page: { w, h }, sections }, null, 1));

  await c.send("Emulation.setDeviceMetricsOverride", {
    width: WIDTH, height: 900, deviceScaleFactor: 2, mobile: false,
  }, sessionId);

  async function clipPNG(x, y, width, height) {
    const shot = await c.send("Page.captureScreenshot", {
      format: "png", captureBeyondViewport: true,
      clip: { x, y, width, height, scale: 1 },
    }, sessionId);
    return Buffer.from(shot.data, "base64");
  }

  const slices = [];
  for (let y = 0; y < h; y += SLICE) {
    const hh = Math.min(SLICE, h - y);
    const buf = await clipPNG(0, y, WIDTH, hh);
    const p = path.join(outdir, `slice-${String(y).padStart(5, "0")}.png`);
    fs.writeFileSync(p, buf);
    slices.push(p);
    console.log("slice", y, `${WIDTH}x${hh}`, buf.length, "bytes");
  }
  fs.writeFileSync(path.join(outdir, "slices.txt"), slices.join("\n") + "\n");
  proc.kill(9);
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });

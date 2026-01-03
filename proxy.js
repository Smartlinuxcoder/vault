import { existsSync } from "fs";

const CERT = "cert.pem";
const KEY = "key.pem";
const TARGET = "http://localhost:8080";

async function ensureCerts() {
  if (existsSync(CERT) && existsSync(KEY)) return;

  console.log("TLS certs not found, generating self-signed cert...");

  const proc = Bun.spawn([
    "openssl",
    "req",
    "-x509",
    "-newkey", "rsa:2048",
    "-nodes",
    "-keyout", KEY,
    "-out", CERT,
    "-days", "365",
    "-subj", "/CN=localhost"
  ], {
    stdout: "inherit",
    stderr: "inherit"
  });

  const code = await proc.exited;
  if (code !== 0) {
    throw new Error("Failed to generate TLS certs");
  }
}

await ensureCerts();

Bun.serve({
  port: 3300,
  hostname: "0.0.0.0",
  tls: {
    key: Bun.file(KEY),
    cert: Bun.file(CERT),
  },

  async fetch(req) {
    const url = new URL(req.url);
    const targetUrl = TARGET + url.pathname + url.search;

    return fetch(targetUrl, {
      method: req.method,
      headers: req.headers,
      body: req.body,
      redirect: "manual",
    });
  },
});

console.log("HTTPS proxy listening on https://0.0.0.0:3300");

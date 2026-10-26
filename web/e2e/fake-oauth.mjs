// A pretend OAuth MCP server for the E2E of the hub's sign-in (P131): the same five endpoints as the one in
// `crates/warden-core/tests/mcp_oauth.rs` (discovery, dynamic registration, authorize, token) plus a bearer-protected `/mcp` that
// answers the MCP handshake and lists no tools. `/authorize` approves by itself and redirects back with a code, because nothing automated can
// click "Allow". It listens on loopback; `stop()` closes it.

import http from "node:http";

export const AUTH_CODE = "e2e-auth-code";
const CLIENT_ID = "e2e-client";
const ACCESS_TOKEN = "e2e-access-token";

function readBody(req) {
  return new Promise((resolve) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => resolve(body));
  });
}

function json(res, status, value, headers = {}) {
  res.writeHead(status, { "content-type": "application/json", ...headers });
  res.end(JSON.stringify(value));
}

/** Starts the server on a free port. Gives `{ url, mcpUrl, tokens, stop() }`; `tokens` counts the code exchanges the hub made. */
export async function startFakeOAuth() {
  const state = { tokens: 0, mcpCalls: 0 };
  const server = http.createServer(async (req, res) => {
    const base = `http://${req.headers.host}`;
    const url = new URL(req.url ?? "/", base);
    if (url.pathname === "/.well-known/oauth-protected-resource") {
      return json(res, 200, { resource: base, authorization_servers: [base] });
    }
    if (url.pathname === "/.well-known/oauth-authorization-server") {
      return json(res, 200, {
        issuer: base,
        authorization_endpoint: `${base}/authorize`,
        token_endpoint: `${base}/token`,
        registration_endpoint: `${base}/register`,
        response_types_supported: ["code"],
        code_challenge_methods_supported: ["S256"],
        grant_types_supported: ["authorization_code", "refresh_token"],
        scopes_supported: ["mcp"],
      });
    }
    if (url.pathname === "/register" && req.method === "POST") {
      const body = JSON.parse((await readBody(req)) || "{}");
      return json(res, 200, { client_id: CLIENT_ID, client_secret: null, client_name: body.client_name, redirect_uris: body.redirect_uris ?? [] });
    }
    if (url.pathname === "/authorize") {
      const redirect = url.searchParams.get("redirect_uri");
      const stateParam = url.searchParams.get("state");
      res.writeHead(303, { location: `${redirect}?code=${AUTH_CODE}&state=${stateParam}` });
      return res.end();
    }
    if (url.pathname === "/token" && req.method === "POST") {
      const form = new URLSearchParams(await readBody(req));
      if (form.get("grant_type") !== "authorization_code" || form.get("code") !== AUTH_CODE) return json(res, 400, { error: "invalid_grant" });
      state.tokens += 1;
      return json(res, 200, { access_token: ACCESS_TOKEN, token_type: "Bearer", expires_in: 3600, refresh_token: "e2e-refresh-token", scope: "mcp" });
    }
    if (url.pathname === "/mcp") {
      if (req.headers.authorization !== `Bearer ${ACCESS_TOKEN}`) {
        res.writeHead(401, { "www-authenticate": `Bearer resource_metadata="${base}/.well-known/oauth-protected-resource"` });
        return res.end();
      }
      state.mcpCalls += 1;
      if (req.method !== "POST") {
        res.writeHead(405);
        return res.end();
      }
      const message = JSON.parse((await readBody(req)) || "{}");
      if (message.id === undefined) {
        res.writeHead(202);
        return res.end();
      }
      if (message.method === "initialize") {
        return json(res, 200, {
          jsonrpc: "2.0",
          id: message.id,
          result: { protocolVersion: message.params?.protocolVersion ?? "2025-03-26", capabilities: { tools: {} }, serverInfo: { name: "fake-sentry", version: "0.0.0" } },
        });
      }
      if (message.method === "tools/list") return json(res, 200, { jsonrpc: "2.0", id: message.id, result: { tools: [] } });
      return json(res, 200, { jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } });
    }
    res.writeHead(404);
    res.end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const root = `http://127.0.0.1:${server.address().port}`;
  return {
    url: root,
    mcpUrl: `${root}/mcp`,
    state,
    stop: () => new Promise((resolve) => server.close(resolve)),
  };
}

/** What the browser does after the person approves: opens the sign-in page without following the redirect and reads where it points. This
 * is the address the person copies from the address bar (the page there does not open, on purpose). */
export async function addressTheBrowserEndsAt(authUrl) {
  const response = await fetch(authUrl, { redirect: "manual" });
  const location = response.headers.get("location");
  if (!location) throw new Error(`The sign-in page did not redirect (status ${response.status}).`);
  return location;
}

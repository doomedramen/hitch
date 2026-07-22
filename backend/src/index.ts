/// <reference types="@cloudflare/workers-types" />

export interface Env {
  HITCH_APP_ID: string;
  HITCH_CLIENT_ID: string;
  HITCH_CLIENT_SECRET: string;
  HITCH_SETUP_SECRET: string;
  HITCH_PRIVATE_KEY: string;
}

interface SetupTokenPayload {
  installation_id: number;
  repo_owner: string;
  repo_name: string;
  exp: number;
}

// ── crypto helpers ─────────────────────────────────────────────────

async function base64url(buf: ArrayBuffer): Promise<string> {
  const base64 = btoa(String.fromCharCode(...new Uint8Array(buf)));
  return base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function importPrivateKey(pem: string): Promise<CryptoKey> {
  const pemHeader = "-----BEGIN RSA PRIVATE KEY-----";
  const pemFooter = "-----END RSA PRIVATE KEY-----";
  const pemContents = pem
    .replace(pemHeader, "")
    .replace(pemFooter, "")
    .replace(/\s/g, "");
  const binary = Uint8Array.from(atob(pemContents), (c) => c.charCodeAt(0));
  return crypto.subtle.importKey(
    "pkcs8",
    binary.buffer,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"]
  );
}

async function signJWT(privateKey: CryptoKey, appId: string): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  const header = { alg: "RS256", typ: "JWT" };
  const payload = {
    iat: now - 60,
    exp: now + 600,
    iss: appId,
  };

  const encodedHeader = await base64url(
    new TextEncoder().encode(JSON.stringify(header))
  );
  const encodedPayload = await base64url(
    new TextEncoder().encode(JSON.stringify(payload))
  );
  const signingInput = `${encodedHeader}.${encodedPayload}`;

  const signature = await crypto.subtle.sign(
    "RSASSA-PKCS1-v1_5",
    privateKey,
    new TextEncoder().encode(signingInput)
  );

  const encodedSignature = await base64url(signature);
  return `${signingInput}.${encodedSignature}`;
}

async function createSetupToken(
  secret: string,
  payload: SetupTokenPayload
): Promise<string> {
  const header = await base64url(
    new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" }))
  );
  const body = await base64url(
    new TextEncoder().encode(JSON.stringify(payload))
  );
  const signingInput = `${header}.${body}`;

  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const signature = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(signingInput)
  );
  const encodedSig = await base64url(signature);
  return `${signingInput}.${encodedSig}`;
}

async function verifySetupToken(
  secret: string,
  token: string
): Promise<SetupTokenPayload | null> {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const [headerB64, payloadB64, sigB64] = parts;

    const key = await crypto.subtle.importKey(
      "raw",
      new TextEncoder().encode(secret),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["verify"]
    );

    const signingInput = `${headerB64}.${payloadB64}`;
    const sig = Uint8Array.from(
      atob(sigB64.replace(/-/g, "+").replace(/_/g, "/")),
      (c) => c.charCodeAt(0)
    );

    const valid = await crypto.subtle.verify(
      "HMAC",
      key,
      sig.buffer,
      new TextEncoder().encode(signingInput)
    );

    if (!valid) return null;

    const payloadJson = atob(payloadB64.replace(/-/g, "+").replace(/_/g, "/"));
    const payload: SetupTokenPayload = JSON.parse(payloadJson);

    if (payload.exp < Math.floor(Date.now() / 1000)) return null;

    return payload;
  } catch {
    return null;
  }
}

// ── GitHub API calls ───────────────────────────────────────────────

async function verifyOAuthToken(token: string): Promise<boolean> {
  const resp = await fetch("https://api.github.com/user", {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "hitch",
    },
  });
  return resp.ok;
}

async function getInstallationId(
  token: string,
  owner: string,
  repo: string
): Promise<number | null> {
  const resp = await fetch(
    `https://api.github.com/repos/${owner}/${repo}/installation`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
        "User-Agent": "hitch",
      },
    }
  );

  if (!resp.ok) return null;

  const data = (await resp.json()) as { id: number };
  return data.id ?? null;
}

async function createInstallationToken(
  privateKey: CryptoKey,
  appId: string,
  installationId: number
): Promise<string | null> {
  const jwt = await signJWT(privateKey, appId);

  const resp = await fetch(
    `https://api.github.com/app/installations/${installationId}/access_tokens`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${jwt}`,
        Accept: "application/vnd.github+json",
        "User-Agent": "hitch",
      },
    }
  );

  if (!resp.ok) return null;

  const data = (await resp.json()) as { token: string };
  return data.token ?? null;
}

// ── CORS headers ───────────────────────────────────────────────────

function corsHeaders(): Record<string, string> {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  };
}

// ── route handlers ─────────────────────────────────────────────────

async function handleSetup(
  request: Request,
  env: Env
): Promise<Response> {
  const body = (await request.json()) as {
    oauth_token: string;
    repo_url: string;
  };

  if (!body.oauth_token || !body.repo_url) {
    return new Response(
      JSON.stringify({ error: "oauth_token and repo_url are required" }),
      { status: 400, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  // Parse repo_url: https://github.com/owner/repo
  const url = new URL(body.repo_url);
  const parts = url.pathname.replace(/\.git$/, "").split("/").filter(Boolean);
  if (parts.length < 2) {
    return new Response(
      JSON.stringify({ error: "Invalid repo_url format" }),
      { status: 400, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }
  const owner = parts[0];
  const repo = parts[1];

  // Verify the OAuth token belongs to the caller
  const valid = await verifyOAuthToken(body.oauth_token);
  if (!valid) {
    return new Response(
      JSON.stringify({ error: "Invalid OAuth token" }),
      { status: 401, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  // Check that the Hitch app is installed on this repo
  const installationId = await getInstallationId(body.oauth_token, owner, repo);
  if (installationId === null) {
    return new Response(
      JSON.stringify({
        error: "Hitch app is not installed on this repository. Install it at https://github.com/apps/hitch/installations/new",
      }),
      { status: 400, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  // Create a setup token (valid for 90 days)
  const payload: SetupTokenPayload = {
    installation_id: installationId,
    repo_owner: owner,
    repo_name: repo,
    exp: Math.floor(Date.now() / 1000) + 90 * 24 * 60 * 60,
  };

  const setupToken = await createSetupToken(env.HITCH_SETUP_SECRET, payload);

  return new Response(
    JSON.stringify({
      setup_token: setupToken,
      installation_id: installationId,
    }),
    { headers: { ...corsHeaders(), "Content-Type": "application/json" } }
  );
}

async function handleToken(
  request: Request,
  env: Env
): Promise<Response> {
  const body = (await request.json()) as { setup_token: string };
  if (!body.setup_token) {
    return new Response(
      JSON.stringify({ error: "setup_token is required" }),
      { status: 400, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  // Verify and decode the setup token
  const payload = await verifySetupToken(env.HITCH_SETUP_SECRET, body.setup_token);
  if (!payload) {
    return new Response(
      JSON.stringify({ error: "Invalid or expired setup token" }),
      { status: 401, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  // Import the private key (cached in the worker's memory between requests)
  const privateKey = await importPrivateKey(env.HITCH_PRIVATE_KEY);

  // Generate a fresh installation access token
  const token = await createInstallationToken(
    privateKey,
    env.HITCH_APP_ID,
    payload.installation_id
  );

  if (!token) {
    return new Response(
      JSON.stringify({ error: "Failed to generate installation token" }),
      { status: 500, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  }

  return new Response(
    JSON.stringify({ token, expires_at: new Date(Date.now() + 3600000).toISOString() }),
    { headers: { ...corsHeaders(), "Content-Type": "application/json" } }
  );
}

// ── main handler ───────────────────────────────────────────────────

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // Handle CORS preflight
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: corsHeaders() });
    }

    const url = new URL(request.url);

    if (request.method === "POST") {
      if (url.pathname === "/setup") {
        return handleSetup(request, env);
      }
      if (url.pathname === "/token") {
        return handleToken(request, env);
      }
    }

    return new Response(
      JSON.stringify({ error: "Not found" }),
      { status: 404, headers: { ...corsHeaders(), "Content-Type": "application/json" } }
    );
  },
};

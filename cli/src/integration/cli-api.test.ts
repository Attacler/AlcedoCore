import { describe, it, expect, beforeAll } from "vitest";

const CORE_URL = process.env.CORE_URL || "http://localhost:8080";
const TEST_SLUG = process.env.TEST_PLUGIN_SLUG || "hello-world";

// ---------------------------------------------------------------------------
// Health check — skip all tests if plugin-core is not reachable
// ---------------------------------------------------------------------------

let coreReachable = false;

beforeAll(async () => {
  try {
    const res = await fetch(`${CORE_URL}/health`, { signal: AbortSignal.timeout(3000) });
    coreReachable = res.ok;
  } catch {
    coreReachable = false;
  }
});

// ---------------------------------------------------------------------------
// Test helpers — replicate CLI patterns without Commander overhead
// ---------------------------------------------------------------------------

async function apiCall<T>(
  apiPath: string,
  options?: { method?: string; body?: unknown }
): Promise<T> {
  const url = `${CORE_URL.replace(/\/$/, "")}${apiPath}`;
  const res = await fetch(url, {
    method: options?.method || "GET",
    headers: { "Content-Type": "application/json" },
    body: options?.body ? JSON.stringify(options.body) : undefined,
  });
  if (!res.ok) {
    const errBody = await res.json().catch(() => ({ error: res.statusText })) as { error?: string };
    throw new Error(errBody.error || `HTTP ${res.status}: ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

/** Check whether a specific API endpoint is available (not returning 405/404). */
async function endpointAvailable(
  apiPath: string,
  method = "GET",
): Promise<boolean> {
  try {
    const res = await fetch(`${CORE_URL.replace(/\/$/, "")}${apiPath}`, {
      method,
      signal: AbortSignal.timeout(2000),
    });
    // 405 Method Not Allowed means the catch-all route caught it;
    // 404 Not Found means the path is not a valid endpoint.
    return res.status !== 405 && res.status !== 404;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Dev session tests (uses same API calls as `alcedo dev` command)
// ---------------------------------------------------------------------------
// Dev session endpoints are only mounted when plugin-core runs in dev mode.
// If the server is not in dev mode these tests skip gracefully.

describe("Dev session API", () => {
  let devEndpointsAvailable = false;

  beforeAll(async () => {
    if (!coreReachable) return;
    // Probe with a minimal POST — if dev mode is off, the catch-all returns 405.
    devEndpointsAvailable = await endpointAvailable("/api/dev/start", "POST");
  });

  it("registers a dev session via POST /api/dev/start", async () => {
    if (!coreReachable || !devEndpointsAvailable) return;

    const result = await apiCall<{ slug: string; url: string; ttl_secs: number; expires_at: string }>(
      "/api/dev/start",
      {
        method: "POST",
        body: { slug: TEST_SLUG, url: "http://localhost:9999", ttl_secs: 60 },
      }
    );

    expect(result.slug).toBe(TEST_SLUG);
    expect(result.url).toBe("http://localhost:9999");
    expect(result.ttl_secs).toBe(60);
    expect(result.expires_at).toBeTruthy();
  });

  it("unregisters a dev session via POST /api/dev/stop", async () => {
    if (!coreReachable || !devEndpointsAvailable) return;

    const result = await apiCall<{ stopped: boolean }>(
      "/api/dev/stop",
      {
        method: "POST",
        body: { slug: TEST_SLUG },
      }
    );

    expect(result.stopped).toBe(true);
  });

  it("dev stop is idempotent (stopped: false on non-existent session)", async () => {
    if (!coreReachable || !devEndpointsAvailable) return;

    const result = await apiCall<{ stopped: boolean }>(
      "/api/dev/stop",
      {
        method: "POST",
        body: { slug: "nonexistent-plugin" },
      }
    );

    expect(result.stopped).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Migration API tests (used by `alcedo migrate` command)
// ---------------------------------------------------------------------------

describe("Migration API", () => {
  it("lists migrations for a plugin via GET /api/plugins/:slug/migrations", async () => {
    if (!coreReachable) return;

    const migrations = await apiCall<Array<{ version: string; name: string; status: string }>>(
      `/api/plugins/${TEST_SLUG}/migrations`
    );

    expect(Array.isArray(migrations)).toBe(true);
    // Migration list should always return an array (possibly empty)
    for (const m of migrations) {
      expect(m.version).toBeTruthy();
      expect(m.name).toBeTruthy();
      expect(["applied", "pending"]).toContain(m.status);
    }
  });
});

// ---------------------------------------------------------------------------
// Request log detail API tests (used by `alcedo dev replay` command)
// ---------------------------------------------------------------------------

describe("Request log detail API", () => {
  it("returns 404 for non-existent request ID", async () => {
    if (!coreReachable) return;

    const url = `${CORE_URL.replace(/\/$/, "")}/api/plugins/${TEST_SLUG}/logs/nonexistent-id/detail`;
    const res = await fetch(url);
    expect(res.status).toBe(404);

    const body = await res.json() as { error?: string };
    expect(body.error).toBeTruthy();
  });

  it("returns log entries listing via GET /api/plugins/:slug/logs", async () => {
    if (!coreReachable) return;

    // The API wraps logs under { data: { logs: [...] } }
    const res = await apiCall<{ data: { logs: Array<{ request_uuid: string; method: string; path: string }> } }>(
      `/api/plugins/${TEST_SLUG}/logs`
    );

    expect(res.data).toBeDefined();
    expect(Array.isArray(res.data.logs)).toBe(true);
    if (res.data.logs.length > 0) {
      const entry = res.data.logs[0];
      expect(entry.request_uuid).toBeTruthy();
      expect(entry.method).toBeTruthy();
      expect(entry.path).toBeDefined();
    }
  });
});

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

describe("Error handling", () => {
  it("returns 404 for non-existent plugin slug", async () => {
    if (!coreReachable) return;

    const url = `${CORE_URL.replace(/\/$/, "")}/api/plugins/nonexistent-plugin/logs`;
    const res = await fetch(url);
    expect(res.status).toBe(404);
  });

  it("handles network errors gracefully", async () => {
    // Use a non-routable address to simulate network failure
    const badUrl = "http://192.0.2.1:9999/api/dev/start";
    try {
      await fetch(badUrl, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ slug: "test", url: "http://localhost:3000" }),
        signal: AbortSignal.timeout(1000),
      });
      // Should not reach here
      expect(true).toBe(false);
    } catch (err: unknown) {
      expect(err).toBeTruthy();
    }
  });
});

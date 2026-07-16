import { describe, it, before, after } from 'node:test';
import assert from 'node:assert';
import { execSync } from 'node:child_process';

const API = 'http://localhost:8080';
const COMPOSE_DIR = '/root/.local/share/opencode/worktree/06feed497ddaf8a4b83d79cd50dad2b3ba1f9b21/happy-falcon';

function psql(sql: string): string {
  return execSync(
    `docker compose exec -T postgres psql -U postgres -d plugin_core`,
    { cwd: COMPOSE_DIR, encoding: 'utf-8', timeout: 10000, input: sql.replace(/\n\s*/g, ' ').trim() },
  );
}

function redisCmd(cmd: string): string {
  return execSync(
    `docker compose exec -T redis redis-cli ${cmd}`,
    { cwd: COMPOSE_DIR, encoding: 'utf-8', timeout: 10000 },
  );
}

function uuid(): string { return crypto.randomUUID(); }

async function api(method: string, path: string, body?: any, headers?: Record<string, string>) {
  const h: Record<string, string> = { 'Content-Type': 'application/json' };
  if (headers) Object.assign(h, headers);
  const res = await fetch(`${API}${path}`, {
    method,
    headers: h,
    body: body ? JSON.stringify(body) : undefined,
    redirect: 'manual',
  });
  const text = await res.text();
  let json: any;
  try { json = JSON.parse(text); } catch { json = text; }
  return { status: res.status, body: json };
}

function createPlugin(slug: string, scopes: string[]) {
  const scopesJson = JSON.stringify(scopes);
  psql(`
    INSERT INTO plugins (slug, image, plugin_type, system_plugin, enabled, env, resources, endpoints, documentation, settings_schema, settings, tags, granted_scopes)
    VALUES ('${slug}', 'plugin-test:latest', 'dynamic', false, true, '{}', '{}', '[]', '[]', '{}', '{}', '[]', '${scopesJson}')
    ON CONFLICT (slug) DO UPDATE SET granted_scopes = '${scopesJson}'
  `);
  psql(`
    INSERT INTO plugin_versions (slug, version, container_id, status, is_active, public_synced, pages_synced)
    VALUES ('${slug}', '1.0.0', 'test-container', 'running', true, false, false)
    ON CONFLICT (slug, version) DO NOTHING
  `);
}

function requestIdHeader(pluginSlug: string): Record<string, string> {
  const rid = uuid();
  redisCmd(`SETEX plugin_req:${rid} 900 ${pluginSlug}`);
  return { 'X-Request-ID': rid };
}

// ---------------------------------------------------------------------------
// Plugin KV scope tests
// ---------------------------------------------------------------------------
describe('Plugin KV scope enforcement', () => {

  it('allows KV get when plugin has kv.get scope', async () => {
    const slug = `pluginkv-${uuid().slice(0, 8)}`;
    createPlugin(slug, ['kv.get', 'kv.put']);

    // Write a key using plugin identity (kv.put)
    let r = await api('PUT', '/api/kv/gottestkey', { value: 'plugin-value' }, requestIdHeader(slug));
    assert.strictEqual(r.status, 200, 'Plugin with kv.put should write');

    // Read using plugin identity (same slug → same KV namespace)
    // Use the same requestIdHeader call which generates a new RID but maps to same slug
    r = await api('GET', '/api/kv/gottestkey', undefined, requestIdHeader(slug));
    if (r.status !== 200) console.log('GET failed:', JSON.stringify(r.body));
    assert.strictEqual(r.status, 200, 'Plugin with kv.get should read');
    // KV get returns {"data": value}
    assert.strictEqual(r.body.data, 'plugin-value');
  });

  it('denies KV get when plugin lacks kv.get scope', async () => {
    const slug = `plugindeny-${uuid().slice(0, 8)}`;
    createPlugin(slug, ['kv.put']);       // has kv.put but NOT kv.get
    // Middleware fails plugin scope check → falls to public role → 401
    const r = await api('GET', '/api/kv/anykey', undefined, requestIdHeader(slug));
    assert.strictEqual(r.status, 401, 'Plugin without kv.get gets 401 from middleware scope check');
  });

  it('allows KV write when plugin has kv.put scope', async () => {
    const slug = `pluginput-${uuid().slice(0, 8)}`;
    createPlugin(slug, ['kv.put']);

    const r = await api('PUT', '/api/kv/writetest', { value: 'written' }, requestIdHeader(slug));
    assert.strictEqual(r.status, 200, 'Plugin with kv.put should write');
  });

  it('denies KV batch_get when plugin lacks kv.batch_get scope', async () => {
    const slug = `pluginbatch-${uuid().slice(0, 8)}`;
    createPlugin(slug, ['kv.get']);       // has kv.get but NOT kv.batch_get
    // Middleware fails plugin scope check → falls to public role → 401
    const r = await api('POST', '/api/kv/batch/get', { keys: ['somekey'] }, requestIdHeader(slug));
    assert.strictEqual(r.status, 401, 'Plugin without kv.batch_get gets 401 from middleware scope check');
  });

  it('returns Unauthorized when X-Request-ID not mapped in Redis', async () => {
    const r = await api('GET', '/api/kv/nonexistent', undefined, { 'X-Request-ID': uuid() });
    assert.strictEqual(r.status, 401, 'Unknown X-Request-ID should return 401');
  });
});

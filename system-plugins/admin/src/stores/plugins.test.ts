import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { usePluginsStore } from './plugins';

const mockPluginsData = [
  { name: 'test-plugin', version: '1.0.0', enabled: true, type: 'user' },
];

vi.mock('../composables/useAlcedoClient', () => ({
  useAlcedoClient: () => ({
    client: {
      plugins: {
        list: vi.fn().mockImplementation(async () => mockPluginsData),
        get: vi.fn().mockImplementation(async () => mockPluginsData[0]),
        enable: vi.fn().mockImplementation(async () => {
          mockPluginsData[0] = { ...mockPluginsData[0], enabled: true };
          return { success: true, message: 'Plugin enabled' };
        }),
        disable: vi.fn().mockImplementation(async () => {
          mockPluginsData[0] = { ...mockPluginsData[0], enabled: false };
          return { success: true, message: 'Plugin disabled' };
        }),
        uninstall: vi.fn().mockResolvedValue({ success: true, message: 'Plugin uninstalled' }),
        install: vi.fn().mockResolvedValue({ name: 'new-plugin', version: '1.0.0', enabled: true, type: 'user' }),
      },
      migrations: {
        list: vi.fn().mockResolvedValue([
          { name: '001_initial', appliedAt: '2024-01-01', pending: false },
        ]),
      },
      settings: {
        get: vi.fn().mockResolvedValue({}),
        update: vi.fn().mockResolvedValue({}),
      },
    },
    isConnected: { value: true },
  }),
}));

describe('usePluginsStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mockPluginsData[0] = { name: 'test-plugin', version: '1.0.0', enabled: true, type: 'user' };
  });

  it('fetchPlugins populates plugins array', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    expect(store.plugins.length).toBeGreaterThan(0);
    expect(store.plugins[0].name).toBe('test-plugin');
  });

  it('enablePlugin calls client.plugins.enable', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    await store.enablePlugin('test-plugin');
    expect(store.plugins[0].status).toBe('enabled');
  });

  it('disablePlugin calls client.plugins.disable', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    await store.disablePlugin('test-plugin');
    expect(store.plugins[0].status).toBe('disabled');
  });

  it('loading state is managed correctly', async () => {
    const store = usePluginsStore();
    expect(store.loading).toBe(false);
    const fetchPromise = store.fetchPlugins();
    expect(store.loading).toBe(true);
    await fetchPromise;
    expect(store.loading).toBe(false);
  });

  it('totalPlugins computed returns correct count', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    expect(store.totalPlugins).toBe(1);
  });

  it('enabledPlugins computed filters correctly', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    expect(store.enabledPlugins.length).toBe(1);
    expect(store.enabledPlugins[0].name).toBe('test-plugin');
  });

  it('disabledPlugins computed returns empty when no disabled plugins', async () => {
    const store = usePluginsStore();
    await store.fetchPlugins();
    expect(store.disabledPlugins.length).toBe(0);
  });
});

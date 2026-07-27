import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createRouter, createMemoryHistory } from 'vue-router';
import PluginDetail from './PluginDetail.vue';
const mockPluginData = {
  name: 'test-plugin',
  version: '1.0.0',
  status: 'enabled' as const,
  plugin_type: 'user' as const,
  description: 'A test plugin',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-02T00:00:00Z',
};

vi.mock('../stores/plugins', () => ({
  usePluginsStore: () => ({
    fetchPluginDetail: vi.fn().mockResolvedValue(mockPluginData),
    fetchPluginSchema: vi.fn().mockResolvedValue({ tables: [] }),
    fetchPluginMigrations: vi.fn().mockResolvedValue({ code: 'ok', migrations: [] }),
    fetchPluginDocs: vi.fn().mockResolvedValue({ plugin: 'test-plugin', docs: [] }),
    fetchPluginPages: vi.fn().mockResolvedValue([]),
    fetchPluginAssets: vi.fn().mockResolvedValue({ css: '', js: '' }),
    fetchPluginSettings: vi.fn().mockResolvedValue({ settings: {}, schema: null }),
    fetchPluginDockerInfo: vi.fn().mockResolvedValue({ data: undefined }),
    fetchPluginVersions: vi.fn().mockResolvedValue({ versions: [] }),
    fetchPluginLogs: vi.fn().mockResolvedValue({ logs: [], next_cursor: null }),
    pluginLoading: false,
    pluginError: null,
    docsLoading: false,
    docsError: null,
    pagesLoading: false,
    settingsLoading: false,
    dockerLoading: false,
    versionsLoading: false,
    logsLoading: false,
    currentPlugin: mockPluginData,
    plugin: mockPluginData,
  }),
}));

vi.mock('vue-router', async () => {
  const actual = await import('vue-router');
  return {
    ...actual,
    useRoute: () => ({ params: { name: 'test-plugin' } }),
    useRouter: () => ({ push: vi.fn() }),
  };
});

const router = createRouter({
  history: createMemoryHistory(),
  routes: [{ path: '/plugins/:name', component: { template: '<div />' } }],
});

describe('PluginDetail.vue', () => {
  it('renders component without error', async () => {
    const wrapper = mount(PluginDetail, {
      global: {
        plugins: [router],
      },
    });
    await wrapper.vm.$nextTick();
    expect(wrapper.exists()).toBe(true);
  });
});

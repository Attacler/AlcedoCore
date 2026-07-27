import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createRouter, createMemoryHistory } from 'vue-router';
import PluginList from './PluginList.vue';
import { usePluginsStore } from '../stores/plugins';

vi.mock('../stores/plugins', () => ({
  usePluginsStore: vi.fn().mockReturnValue({
    plugins: [
      { name: 'test-plugin', version: '1.0.0', status: 'enabled', plugin_type: 'user' },
    ],
    loading: false,
    error: null,
    fetchPlugins: vi.fn(),
    totalPlugins: 1,
    enabledPlugins: [],
    disabledPlugins: [],
  }),
}));

vi.mock('../composables/useToast', () => ({
  useToast: () => ({
    show: vi.fn(),
  }),
}));

const router = createRouter({
  history: createMemoryHistory(),
  routes: [{ path: '/', component: { template: '<div />' } }],
});

describe('PluginList.vue', () => {
  it('renders plugin list', async () => {
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain('test-plugin');
  });

  it('shows loading state when loading is true', async () => {
    const store = usePluginsStore();
    store.loading = true;
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    expect(wrapper.find('.bg-white').exists()).toBe(true);
  });

  it('renders empty state when no plugins', async () => {
    const store = usePluginsStore();
    store.plugins = [];
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain('No plugins found');
  });

  it('has Add Plugin button', async () => {
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    expect(wrapper.text()).toContain('Add Plugin');
  });

  it('has search input', async () => {
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    expect(wrapper.find('input').exists()).toBe(true);
    expect(wrapper.find('input').attributes('placeholder')).toBe('Search plugins...');
  });

  it('has filter toggle buttons', async () => {
    const wrapper = mount(PluginList, {
      global: {
        plugins: [router],
      },
    });
    const buttons = wrapper.findAll('button');
    const filterButtons = buttons.filter(b => ['All', 'Enabled', 'Disabled', 'System', 'User'].includes(b.text()));
    expect(filterButtons.length).toBe(5);
  });
});

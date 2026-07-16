import AutomationDashboard from './AutomationDashboard.vue'
import FunctionEditor from './FunctionEditor.vue'
import ExecutionLogs from './ExecutionLogs.vue'
import TriggerList from './TriggerList.vue'
import TriggerDetail from './TriggerDetail.vue'

export { AutomationDashboard, FunctionEditor, ExecutionLogs, TriggerList, TriggerDetail }

// Inject PrimeIcons CSS + missing PrimeVue component CSS variables
// PrimeVue only injects styles for components the admin app uses — our
// dynamically loaded components (Select, Tabs, etc.) need their CSS too.
(function() {
  // 1. PrimeIcons
  if (!document.querySelector('style[data-pi="true"]')) {
    const s = document.createElement('style')
    s.setAttribute('data-pi', 'true')
    s.textContent = `@font-face{font-family:"PrimeIcons";src:url("https://cdn.jsdelivr.net/npm/primeicons@7.0.0/fonts/primeicons.eot");src:url("https://cdn.jsdelivr.net/npm/primeicons@7.0.0/fonts/primeicons.eot?#iefix") format("embedded-opentype"),url("https://cdn.jsdelivr.net/npm/primeicons@7.0.0/fonts/primeicons.woff2") format("woff2"),url("https://cdn.jsdelivr.net/npm/primeicons@7.0.0/fonts/primeicons.woff") format("woff"),url("https://cdn.jsdelivr.net/npm/primeicons@7.0.0/fonts/primeicons.ttf") format("truetype");font-display:swap}.pi{font-family:"PrimeIcons";speak:none;font-style:normal;font-weight:400;font-variant:normal;text-transform:none;line-height:1;display:inline-block;-webkit-font-smoothing:antialiased;-moz-osx-font-smoothing:grayscale}.pi-chevron-down:before{content:"\\e900"}.pi-save:before{content:"\\e9b2"}.pi-play:before{content:"\\e9c3"}.pi-plus:before{content:"\\e95b"}.pi-check:before{content:"\\e909"}.pi-spinner:before{content:"\\e9b4"}`
    document.head.appendChild(s)
  }
  // 2. Missing PrimeVue component CSS variables (not injected by admin's PrimeVue config)
  if (!document.querySelector('style[data-pv-tabs="true"]')) {
    const s = document.createElement('style')
    s.setAttribute('data-pv-tabs', 'true')
    s.textContent = `
:root {
  --p-select-background: #ffffff;
  --p-select-border-color: #d1d5db;
  --p-select-border-radius: 6px;
  --p-select-padding-x: 0.75rem;
  --p-select-padding-y: 0.5rem;
  --p-select-transition-duration: 0.2s;
  --p-select-color: #374151;
  --p-select-focus-border-color: #3b82f6;
  --p-select-focus-ring-width: 0;
  --p-select-focus-ring-style: none;
  --p-select-focus-ring-color: transparent;
  --p-select-focus-ring-offset: 0;
  --p-select-focus-ring-shadow: none;
  --p-select-overlay-background: #ffffff;
  --p-select-overlay-border-color: #d1d5db;
  --p-select-overlay-border-radius: 6px;
  --p-select-overlay-shadow: 0 4px 6px -1px rgba(0,0,0,0.1);
  --p-select-option-padding-x: 0.75rem;
  --p-select-option-padding-y: 0.5rem;
  --p-select-option-color: #374151;
  --p-select-option-hover-background: #f3f4f6;
  --p-select-option-hover-color: #374151;
  --p-select-option-selected-background: #eff6ff;
  --p-select-option-selected-color: #1d4ed8;
  --p-inputtext-background: #ffffff;
  --p-inputtext-border-color: #d1d5db;
  --p-inputtext-border-radius: 6px;
  --p-inputtext-padding-x: 0.75rem;
  --p-inputtext-padding-y: 0.5rem;
  --p-inputtext-color: #374151;
  --p-inputtext-transition-duration: 0.2s;
  --p-tablist-background: transparent;
  --p-tablist-border-color: #e5e7eb;
  --p-tab-padding: 0.75rem 1.25rem;
  --p-tab-font-weight: 600;
  --p-tab-color: #6b7280;
  --p-tab-active-color: #3b82f6;
  --p-tab-active-border-color: #3b82f6;
  --p-tab-hover-color: #374151;
  --p-tab-hover-border-color: #e5e7eb;
  --p-tabpanel-padding: 0;
  --p-content-background: transparent;
  --p-content-border-color: #e5e7eb;
  --p-content-color: #374151;
}
`
    document.head.appendChild(s)
  }
})()

export default {
  manifestVersion: 1,
  pluginSlug: 'automation',
  pages: [
    { path: '/functions', label: 'Functions', icon: 'code', sidebar: true, component: AutomationDashboard },
    { path: '/functions/new', label: 'New Function', icon: 'add', sidebar: false, fullpage: true, component: FunctionEditor },
    { path: '/functions/edit', label: 'Edit Function', icon: 'edit', sidebar: false, fullpage: true, component: FunctionEditor },
    { path: '/triggers', label: 'Triggers', icon: 'pi-sync', sidebar: true, component: TriggerList },
    { path: '/triggers/new', label: 'New Trigger', icon: 'add', sidebar: false, component: TriggerDetail },
    { path: '/triggers/edit', label: 'Edit Trigger', icon: 'edit', sidebar: false, component: TriggerDetail },
    { path: '/execution-logs', label: 'Execution Logs', icon: 'history', sidebar: true, component: ExecutionLogs },
  ],
}

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import Components from 'unplugin-vue-components/vite'
import { PrimeVueResolver } from 'unplugin-vue-components/resolvers'
import { resolve } from 'path'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [
    tailwindcss(),
    vue(),
    Components({
      resolvers: [PrimeVueResolver()],
    }),
  ],

  // D-07: Output build artifacts to public/ directory (not default dist/)
  root: '.',
  base: '/admin/',
  build: {
    outDir: 'public',
    emptyOutDir: true,
  },

  // D-08: Dev server proxies /plugins/* API calls to host server
  server: {
    port: 3000,
    proxy: {
      '/plugins': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },

  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
})

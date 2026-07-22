import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import Components from "unplugin-vue-components/vite";
import { PrimeVueResolver } from "unplugin-vue-components/resolvers";
import { resolve } from "path";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
    plugins: [
        tailwindcss(),
        vue(),
        Components({
            resolvers: [PrimeVueResolver()],
        }),
    ],

    root: ".",
    base: "/admin/",
    build: {
        outDir: "public",
        emptyOutDir: true,
    },

    server: {
        port: 3000,
        proxy: {
            "/plugins": {
                target: "http://localhost:8080",
                changeOrigin: true,
            },
            "/api": {
                target: "http://localhost:8080",
                changeOrigin: true,
            },
        },
    },

    resolve: {
        alias: {
            "@": fileURLToPath(new URL("./src", import.meta.url)),
        },
    },
});

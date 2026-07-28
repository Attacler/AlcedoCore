import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import Components from "unplugin-vue-components/vite";
import { PrimeVueResolver } from "unplugin-vue-components/resolvers";
import { fileURLToPath, URL } from "node:url";
import { resolve } from "node:path";

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
            "/p": {
                target: "http://localhost:8080",
                changeOrigin: true,
            },
        },
    },

    resolve: {
        alias: {
            "@": fileURLToPath(new URL("./src", import.meta.url)),
            "alcedocore-sdk-node": resolve(
                __dirname,
                "../../sdk/alcedocore-sdk-node/src",
            ),
        },
    },
});

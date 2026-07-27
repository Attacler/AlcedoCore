import tailwindcss from "@tailwindcss/vite";

export default defineNuxtConfig({
    ssr: true,
    devServer: { port: 8080 },
    app: { baseURL: "/hello-world-nuxt/" },
    css: ["~/assets/css/main.css"],
    vite: {
        plugins: [tailwindcss()],
    },
    nitro: {
        preset: "node-server",
        output: { serverDir: ".output/server" },
        externals: { inline: ["alcedo-sdk-node"] },
    },
});

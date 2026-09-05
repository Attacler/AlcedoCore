import { ref, readonly } from "vue";
import { createClient } from "@alcedocore/sdk";

const isConnected = ref(true);

export function useAlcedoClient(devServerUrl?: string) {
    const client = createClient(
        typeof window !== "undefined" ? window.location.origin : "",
    );

    const assets = devServerUrl
        ? {
              devMode: (name: string) => ({
                  js: `${devServerUrl}/dev/js?plugin=${encodeURIComponent(name)}`,
                  css: `${devServerUrl}/dev/css?plugin=${encodeURIComponent(name)}`,
              }),
          }
        : null;

    return {
        client,
        isConnected: readonly(isConnected),
        assets,
    };
}

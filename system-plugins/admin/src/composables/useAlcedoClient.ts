import { ref, readonly, reactive } from "vue";
import { createClient, type ClientOptions } from "@alcedocore/sdk";

const isConnected = ref(true);

export function useAlcedoClient(devServerUrl?: string) {
    const sdkOptions = reactive<ClientOptions>({});
    const client = createClient(
        typeof window !== "undefined" ? window.location.origin : "",
        sdkOptions,
    );

    function setSdkContext(app?: string, version?: string) {
        sdkOptions.app = app;
        sdkOptions.version = version;
    }

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
        setSdkContext,
        isConnected: readonly(isConnected),
        assets,
    };
}

import { ref, readonly } from "vue";
import {
    createClient,
    AlcedoError,
    NotFoundError,
    ValidationError,
    AuthenticationError,
    ServerError,
    ConnectionError,
} from "alcedocore-sdk-node";

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

export {
    AlcedoError,
    NotFoundError,
    ValidationError,
    AuthenticationError,
    ServerError,
    ConnectionError,
};

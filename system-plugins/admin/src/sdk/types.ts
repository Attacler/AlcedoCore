import type { InjectionKey } from "vue";

type SdkClient = {
    plugins: {
        list: () => Promise<any>;
        get: (name: string) => Promise<any>;
        enable: (name: string) => Promise<any>;
        disable: (name: string) => Promise<any>;
        deploy: (name: string, tag: string) => Promise<any>;
        pages: (name: string) => Promise<any>;
        assets: (name: string) => Promise<any>;
        schema: (name: string) => Promise<any>;
        docs: (name: string) => Promise<any>;
        docContent: (name: string, path: string) => Promise<any>;
        versions: (name: string) => Promise<any>;
        create: (formData: FormData) => Promise<any>;
        delete: (name: string) => Promise<any>;
        docker: (name: string) => Promise<any>;
    };
    collections: Record<string, (...args: any[]) => Promise<any>>;
    settings: Record<string, (...args: any[]) => Promise<any>>;
    migrations: Record<string, (...args: any[]) => Promise<any>>;
    registries: Record<string, (...args: any[]) => Promise<any>>;
    activityLogs: {
        listCollections: (params?: URLSearchParams) => Promise<any>;
        listSystem: (params?: URLSearchParams) => Promise<any>;
    };
};

export interface AlcedoStores {
    plugins: object;
    collections: object;
    registries: object;
    theme: object;
}

export interface AlcedoSDK {
    client: SdkClient;
    stores: AlcedoStores;
    signal?: AbortSignal;
    navigate: (path: string) => void;
    toast: {
        show: (
            message: string,
            type: "info" | "success" | "warning" | "error",
            duration?: number,
        ) => void;
    };
}

export interface PageSDK {
    client: AlcedoSDK["client"];
    stores: AlcedoSDK["stores"];
    navigate: (path: string) => void;
    toast: AlcedoSDK["toast"];
    signal?: AbortSignal;
}

export interface InputWidgetSDK {
    client: AlcedoSDK["client"];
    stores: Pick<AlcedoSDK["stores"], "theme">;
}

export interface ViewTypeSDK {
    client: AlcedoSDK["client"];
    stores: Pick<AlcedoSDK["stores"], "collections" | "plugins" | "theme">;
    navigate: (path: string) => void;
}

export const ALCEDO_SDK_KEY: InjectionKey<AlcedoSDK> = Symbol(
    "alcedocore-sdk-node",
);

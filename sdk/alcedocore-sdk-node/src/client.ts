import ky, { type AfterResponseHook, type BeforeRequestHook } from "ky";
import { createPluginsResource } from "./plugins.js";
import { createHealthResource } from "./health.js";
import { createMigrationsResource } from "./migrations.js";
import { createSettingsResource } from "./settings.js";
import { createUsageResource } from "./usage.js";
import { createKvResource } from "./kv.js";
import { createDbResource } from "./db.js";
import { createSchemaResource } from "./schema.js";
import { createLogsResource } from "./logs.js";
import { createAuthResource } from "./auth.js";
import { createSessionsResource } from "./sessions.js";
import { createAppsResource } from "./apps.js";
import { createVersionsResource } from "./versions.js";
import { createUsersResource } from "./users.js";
import { createRolesResource } from "./roles.js";
import { createPoliciesResource } from "./policies.js";
import { createCollectionsResource } from "./collections.js";
import { createItemsResource } from "./items.js";
import { createFilesResource } from "./files.js";
import { createRegistriesResource } from "./registries.js";
import { createActivityLogsResource } from "./activityLogs.js";
import { createAppSettingsResource } from "./appSettings.js";
import { createDeveloperApiKeysResource } from "./developerApiKeys.js";
import { AlcedoApiError, type JSendResponse } from "./types/jsend.js";

const DEFAULT_TIMEOUT = 30_000;
const JSON_CONTENT_TYPE = "application/json";

function isJSendEnvelope(value: unknown): value is JSendResponse<unknown> {
    if (typeof value !== "object" || value === null || !("status" in value)) {
        return false;
    }
    const status = (value as { status: unknown }).status;
    return status === "success" || status === "fail" || status === "error";
}

/**
 * Global ky `afterResponse` hook that understands AlcedoCore's JSend envelope.
 *
 * - `status: "success"` → the response body is replaced with `data`, so every
 *   `.json()` call site receives the payload directly.
 * - `status: "fail" | "error"` → throws {@link AlcedoApiError} carrying the
 *   JSend `message`/`code`.
 * - Non-JSON responses (file downloads, docs, empty bodies) pass through
 *   untouched.
 */
async function unwrapJSend(response: Response): Promise<Response> {
    const contentType = response.headers.get("content-type") ?? "";
    if (!contentType.includes(JSON_CONTENT_TYPE)) return response;

    const raw = await response.clone().text();
    if (!raw) return response;

    let parsed: unknown;
    try {
        parsed = JSON.parse(raw);
    } catch {
        return response;
    }

    if (!isJSendEnvelope(parsed)) return response;

    if (parsed.status !== "success") {
        throw new AlcedoApiError(
            parsed.message ?? "Request failed",
            parsed.code,
            parsed.status,
            response.status,
        );
    }

    return new Response(JSON.stringify(parsed.data ?? null), {
        status: response.status,
        statusText: response.statusText,
        headers: { "content-type": JSON_CONTENT_TYPE },
    });
}

export interface ClientOptions {
    timeout?: number;
    requestId?: string;
    app?: string;
    version?: string;
    retry?: {
        limit?: number;
        delay?: (attempt: number) => number;
    };
}

export interface AppHeaderOptions {
    app?: string;
    version?: string;
}

export function resolveAppHeaders(
    defaults: AppHeaderOptions | undefined,
    perCall: AppHeaderOptions | undefined,
    headers: Headers,
) {
    const app = perCall?.app ?? defaults?.app;
    const version = perCall?.version ?? defaults?.version;
    if (app) headers.set("X-App", app);
    if (version) headers.set("X-Version", version);
}

export function createClient(baseUrl: string, options: ClientOptions = {}) {
    const kyInstance = ky.create({
        baseUrl: baseUrl.replace(/\/$/, ""),
        prefix: "/api",
        timeout: options.timeout ?? DEFAULT_TIMEOUT,
        retry: {
            limit: options.retry?.limit ?? 3,
            delay:
                options.retry?.delay ??
                ((attempt: number) => Math.pow(2, attempt) * 1000),
            methods: ["get", "post", "put", "delete", "patch"] as any,
            statusCodes: [408, 413, 429, 500, 502, 503, 504],
        },
        hooks: {
            beforeRequest: [
                ((state: any) => {
                    const rid = state.options?.requestId ?? options.requestId;

                    if (rid) state.request.headers.set("X-Request-ID", rid);

                    resolveAppHeaders(
                        options,
                        state.options,
                        state.request.headers as Headers,
                    );
                }) as BeforeRequestHook,
            ],
            afterResponse: [
                (({ response }: { response: Response }) =>
                    unwrapJSend(response)) as AfterResponseHook,
            ],
        },
    });

    function createRequest(ky2: typeof kyInstance) {
        return async (method: string, path: string, opts?: any) => {
            return (ky2 as any)[method](path, opts);
        };
    }

    return {
        plugins: createPluginsResource(kyInstance),
        health: createHealthResource(kyInstance),
        activityLogs: createActivityLogsResource(kyInstance),
        appSettings: createAppSettingsResource(kyInstance),
        developerApiKeys: createDeveloperApiKeysResource(kyInstance),
        registries: createRegistriesResource(kyInstance),
        migrations: createMigrationsResource(kyInstance),
        settings: createSettingsResource(kyInstance),
        usage: createUsageResource(kyInstance),
        kv: createKvResource(kyInstance),
        db: createDbResource(kyInstance),
        schema: createSchemaResource(kyInstance),
        logs: createLogsResource(kyInstance),
        auth: createAuthResource(kyInstance),
        sessions: createSessionsResource(kyInstance),
        apps: createAppsResource(kyInstance),
        versions: createVersionsResource(kyInstance),
        users: createUsersResource(kyInstance),
        roles: createRolesResource(kyInstance),
        policies: createPoliciesResource(kyInstance),
        collections: createCollectionsResource(kyInstance),
        items: createItemsResource(kyInstance),
        files: createFilesResource(kyInstance),
        request: createRequest(kyInstance),
    };
}

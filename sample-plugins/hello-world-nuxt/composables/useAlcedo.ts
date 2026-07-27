import { createClient } from "alcedo-sdk-node";

const CORE_URL = process.env.CORE_URL || "http://core:8080";

export const alcedo = createClient(CORE_URL, {
    timeout: 30_000,
    retry: { limit: 2 },
});

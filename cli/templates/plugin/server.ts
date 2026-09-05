import express from "express";
import { createClient } from "@alcedocore/sdk";

const app = express();
const port = process.env.PORT || 3000;
const CORE_URL = process.env.CORE_URL || "http://localhost:8080";
const PLUGIN_SLUG = process.env.PLUGIN_SLUG || "<%= slug %>";

const client = createClient(CORE_URL, {
    timeout: 30_000,
    retry: { limit: 2 },
});

app.get("/", (req, res) => {
    res.send("Hello World!");
});

app.get("/settings", async (_req, res) => {
    try {
        const settings = await client.settings.get(PLUGIN_SLUG, {
            headers: {
                ["x-request-id"]: _req.headers["x-request-id"],
            },
        });
        res.json({ slug: PLUGIN_SLUG, settings });
    } catch (err: any) {
        res.status(500).json({ slug: PLUGIN_SLUG, error: err.message });
    }
});

app.listen(port, () => {
    console.log(`Example app listening on port ${port}`);
});

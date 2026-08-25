import { Command } from "commander";
import express from "express";
import EventEmitter from "node:events";
import { buildFrontendFiles, currentImportsGlobalVue } from "./build-frontend";
import chokidar from "chokidar";

export const serveFrontendCommand = new Command("serve-frontend")
    .description("Compile all pages into a dist directory")
    .action(async (opts: null, cmd: Command) => {
        const app = express();

        app.use((req, res, next) => {
            res.header(`Access-Control-Allow-Origin`, `*`);
            res.header(`Access-Control-Allow-Methods`, `GET`);
            res.header(`Access-Control-Allow-Headers`, `Content-Type`);
            next();
        });

        app.get("/dev/css", async (req, res) => {
            res.end(lastCSS);
        });
        app.get("/dev/js", async (req, res) => {
            res.json({
                code: lastJS,
                currentImportsGlobalVue: [...currentImportsGlobalVue].map((e) =>
                    e.trim(),
                ),
            });
        });

        const updateEvents = new EventEmitter();

        const watchUpdates = chokidar.watch("./pages");

        app.get("/streaming", (req, res) => {
            res.setHeader("Cache-Control", "no-cache");
            res.setHeader("Content-Type", "text/event-stream");
            res.setHeader("Access-Control-Allow-Origin", "*");
            res.setHeader("Connection", "keep-alive");
            res.flushHeaders(); // flush the headers to establish SSE with client

            updateEvents.on("jsUpdate", () => {
                res.write(`id:0\nevent:reloadJS\ndata:js\n\n`);
            });
            updateEvents.on("cssUpdate", () => {
                res.write(`id:0\nevent:reloadCSS\ndata:css\n\n`);
            });
            // If client closes connection, stop sending events
            res.on("close", () => {
                // watcher.
                res.end();
            });
        });

        buildInMemory().then(() => {
            app.listen(3003, () => {
                console.log("Dev server is running");

                watchUpdates.on("all", (event, path) => {
                    let beforeTSJS = tsJS;
                    let beforeTSCSS = tsCSS;

                    buildInMemory().then(() => {
                        if (beforeTSCSS != tsCSS) {
                            updateEvents.emit("cssUpdate");
                        }
                        if (beforeTSJS != tsJS) {
                            updateEvents.emit("jsUpdate");
                        }
                    });
                });
            });
        });
    });

let lastCSS = "",
    lastJS = "",
    tsJS = new Date().getTime(),
    tsCSS = new Date().getTime();

export async function buildInMemory() {
    console.log("Building...");
    const result = await buildFrontendFiles(false);

    const js = result
        .outputFiles!.filter((e) => e.path.endsWith(".js"))
        .map((e) => e.text)
        .join("\n");
    const css = `${result
        .outputFiles!.filter((e) => e.path.endsWith(".css"))
        .map((e) => e.text)
        .join("\n")}\`;`;

    if (js != lastJS) {
        tsJS = new Date().getTime();
        lastJS = js;
    }
    if (css != lastCSS) {
        tsCSS = new Date().getTime();
        lastCSS = css;
    }
    console.log("Build done!");
}

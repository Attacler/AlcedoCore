import esbuild from "esbuild";
// @ts-ignore - unplugin-vue exports ./esbuild but types are not fully resolved
import Vue from "unplugin-vue/esbuild";
import postcssPlugin from "@chialab/esbuild-plugin-postcss";
import fs from "node:fs";
import path from "path";
import { fileURLToPath } from "node:url";
import { cwd } from "node:process";
import { Command } from "commander";

export const compilePagesCommand = new Command("compile-pages")
    .description("Compile all pages into a dist directory")
    .action(async (opts: null, cmd: Command) => {
        renderPages().catch((err) => {
            console.error(`[page-compiler] Failed:`, err);
            process.exit(1);
        });
    });
console.log(process.cwd());
const compilerDir = path.dirname(process.cwd());
const compilerRootDir = path.join(compilerDir, "..");
const compilerNodeModules = path.join(compilerRootDir, "node_modules");

interface CliArgs {
    plugin: string;
    output: string;
}

function parseArgs(): CliArgs {
    let plugin = "./pages";
    let output = "./pages/dist";

    for (let i = 0; i < process.argv.length; i++) {
        const arg = process.argv[i];
        if (arg === "--plugin" && i + 1 < process.argv.length) {
            plugin = process.argv[i + 1];
        } else if (arg.startsWith("--plugin=")) {
            plugin = arg.split("=")[1];
        }
        if (arg === "--output" && i + 1 < process.argv.length) {
            output = process.argv[i + 1];
        } else if (arg.startsWith("--output=")) {
            output = arg.split("=")[1];
        }
    }

    return { plugin, output };
}

let currentImportsGlobalVue = new Set<string>();
let importingModules =
    "createTextVNode,defineComponent,toDisplayString,createElementVNode,openBlock,createElementBlock,Fragment,renderSlot,createCommentVNode,useModel,vModelText,vModelSelect,withDirectives,withKeys,normalizeClass,renderList,computed,ref,onMounted,normalizeStyle,createComment,resolveComponent,resolveDirective,withCtx,toHandlers,mergeProps,createSlots,withScopeId,createStaticVNode,createBlock,popScopeId,pushScopeId,isRef,unref,isReactive,toRef,toRefs,isProxy,isReadonly,shallowRef,triggerRef,customRef,markRaw,toRaw,reactive,readonly,watch,watchEffect,watchPostEffect,watchSyncEffect,provide,inject,getCurrentInstance,h,nextTick,onBeforeMount,onBeforeUpdate,onBeforeUnmount,onUpdated,onUnmounted,onActivated,onDeactivated,onErrorCaptured,onRenderTracked,onRenderTriggered,isVNode,cloneVNode,createVNode,Transition,TransitionGroup,Teleport,Suspense,KeepAlive,defineAsyncComponent,defineEmits,defineExpose,defineProps,withDefaults,useAttrs,useSlots,useCssModule,useCssVars,EffectScope,effectScope,getCurrentScope,onScopeDispose,useId,resolveDynamicComponent,normalizeProps,withModifiers,vShow";
currentImportsGlobalVue = new Set(importingModules.split(","));
const vueGlobalPlugin = {
    name: "vue-global",
    setup(build: any) {
        build.onResolve({ filter: /^vue$/ }, async (args: any) => {
            let source = await fs.promises.readFile(args.importer, "utf8");

            const matchingImports =
                source.match(/import\s*{\s*([^}]*)\s*}\s*from\s*["']vue["']/) ||
                [];
            // console.log(matchingImports);

            if (matchingImports[1]) {
                importingModules = matchingImports[1] + "," + importingModules;
                for (const imp of importingModules.split(",")) {
                    currentImportsGlobalVue.add(imp.trim());
                }
            }
            return {
                path: args.path,
                namespace: "vue-global",
            };
        });

        build.onLoad({ filter: /.*/, namespace: "vue-global" }, () => {
            return {
                contents: `
            export const { ${[...currentImportsGlobalVue].join(",")} } = window.vue;
            export default window.vue;
            `,
                loader: "js",
            };
        });
    },
};

async function renderPages(): Promise<void> {
    const args = parseArgs();
    const pluginPath = path.resolve(process.cwd(), args.plugin);
    const outputDir = path.resolve(process.cwd(), args.output);
    const mainPath = path.join(pluginPath, "main.ts");

    console.log(`[page-compiler] Compiling pages from: ${pluginPath}`);
    console.log(`[page-compiler] Output directory: ${outputDir}`);

    if (!fs.existsSync(mainPath)) {
        console.error(`[page-compiler] Entry point not found: ${mainPath}`);
        process.exit(1);
    }

    await fs.promises.mkdir(outputDir, { recursive: true });

    const buildOptions: esbuild.BuildOptions = {
        entryPoints: [mainPath],
        bundle: true,
        format: "esm",
        target: "es2020",
        platform: "browser",
        sourcemap: false,
        minify: false,
        outdir: outputDir,
        plugins: [postcssPlugin(), vueGlobalPlugin, Vue({ sourceMap: false })],
        alias: {
            vue: path.join(
                compilerNodeModules,
                "vue",
                "dist",
                "vue.esm-bundler.js",
            ),
        },
        loader: {
            ".css": "css",
        },
        write: true,
        external: ["vue"],
    };

    await esbuild.build(buildOptions);

    const mainJsPath = path.join(outputDir, "main.js");
    const pluginPagesJsPath = path.join(outputDir, "plugin-pages.js");

    if (fs.existsSync(mainJsPath)) {
        await fs.promises.rename(mainJsPath, pluginPagesJsPath);
        console.log(`[page-compiler] Renamed main.js -> plugin-pages.js`);
    }

    console.log(`[page-compiler] Build complete`);
}

<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRouter } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import Button from "primevue/button";
import Select from "primevue/select";
import RadioButton from "primevue/radiobutton";
import Checkbox from "primevue/checkbox";
import type { JsonSchemaProperty } from "@/types/dynamic-form";
import {
    Accordion,
    AccordionContent,
    AccordionPanel,
    AccordionHeader,
} from "primevue";
import DynamicFormField from "@/components/plugins/DynamicFormField.vue";

interface ImageListItem {
    name: string;
    tag: string;
    created_at?: string;
}
interface ManifestScope {
    name: string;
    description?: string;
}

const steps = [
    "Registry & Plugin",
    "Version",
    "Preview",
    "Configure",
    "Install",
];
const currentStep = ref(0);

const { client } = useAlcedoClient(),
    store = usePluginsStore(),
    toast = useToast(),
    router = useRouter();

const registries = ref<Array<{ id: number; name: string; url: string }>>([]),
    selectedRegistryId = ref(""),
    images = ref<ImageListItem[]>([]),
    imagesLoading = ref(false),
    selectedRepo = ref<string | null>(null),
    selectedTag = ref<string | null>(null);

const groupedRepos = computed(() => {
    const map = new Map<string, { images: ImageListItem[] }>();
    for (const img of images.value) {
        const entry = map.get(img.name) || { images: [] };
        entry.images.push(img);
        map.set(img.name, entry);
    }
    return Array.from(map.entries()).map(([name, info]) => ({
        name,
        latestTag:
            info.images.sort((a, b) =>
                (b.created_at || "").localeCompare(a.created_at || ""),
            )[0]?.tag || "latest",
        tagCount: info.images.length,
    }));
});

const versions = computed(() => {
    if (!selectedRepo.value) return [];
    // console.log(images.value)
    return images.value
        .filter((i) => i.name === selectedRepo.value)
        .map((i) => ({
            tag: i.tag || "latest",
            created_at: i.created_at,
        }))
        .sort((a, b) => {
            if (a.tag == "latest") return 1;

            return a.tag.localeCompare(b.tag) * -1;
        });
    // TODO add propper sort based on version
});

const previewManifest = ref<any>(null);
const previewLoading = ref(false);
const previewError = ref("");
const previewMigrations = ref<string[]>([]);
const grantRootAccess = ref(false);

const configSettings = ref<Record<string, any>>({});
const installMode = ref<"install" | "install-and-start">("install-and-start");

const settingsSchema = computed(
    () => previewManifest.value?.settings_schema || null,
);
const manifestScopes = computed<ManifestScope[]>(
    () => previewManifest.value?.scopes || [],
);

const pluginSlug = ref("");
const installPhase = ref<"pulling" | "done" | "error">("pulling");
const installError = ref("");
const phaseLabels: Record<string, string> = {
    pulling: "Deploying plugin...",
    done: "Done!",
    error: "Error",
};
const phaseOrder: string[] = ["pulling", "done"];
const installPhaseLabel = computed(() => phaseLabels[installPhase.value]);
const installProgress = computed(() => {
    const i = phaseOrder.indexOf(installPhase.value);
    return i >= 0 ? (i / (phaseOrder.length - 1)) * 100 : 0;
});

function getImageRef(): string {
    const reg = registries.value.find(
        (r) => String(r.id) === selectedRegistryId.value,
    );
    const rawHost = reg ? reg.url.replace(/^https?:\/\//, "") : "";
    // Always use localhost:5000 for the local registry — Docker treats
    // non-localhost as HTTPS-only, but the dev registry is HTTP only.
    const port = rawHost.split(":")[1];
    const isLocal = port === "5000";
    const host = isLocal ? "localhost:5000" : rawHost;
    return `${host}/${selectedRepo.value}:${selectedTag.value}`;
}

function stepClass(idx: number): Record<string, boolean> {
    return {
        "bg-blue-500 text-white": currentStep.value === idx,
        "bg-blue-100 text-blue-700": currentStep.value > idx,
        "bg-gray-50 text-gray-400 hover:text-gray-600": currentStep.value < idx,
        "border-r border-gray-200": idx < steps.length - 1,
    };
}

async function fetchRegistries() {
    try {
        const r = await client.registries.list();
        registries.value = r.data?.registries || [];
    } catch (e) {
        toast.show("Failed to fetch registries", "error");
    }
}

async function onRegistryChange() {
    selectedRepo.value = null;
    selectedTag.value = null;
    previewManifest.value = null;
    if (!selectedRegistryId.value) {
        images.value = [];
        return;
    }
    imagesLoading.value = true;
    try {
        const r = await client.registries.images(
            Number(selectedRegistryId.value),
        );
        images.value = r.data?.images || [];
    } catch (e) {
        images.value = [];
        toast.show("Failed to fetch images from registry", "error");
    } finally {
        imagesLoading.value = false;
    }
}

function selectRepo(name: string) {
    if (selectedRepo.value === name) return;
    selectedRepo.value = name;
    selectedTag.value = null;
    previewManifest.value = null;
    previewMigrations.value = [];
    configSettings.value = {};
    const [latestTag] = versions.value;

    if (latestTag) {
        selectedTag.value = latestTag.tag;
    }
}

async function goToPreview() {
    currentStep.value = 2;
    await fetchPreview();
}

async function fetchPreview() {
    const ref = getImageRef();
    previewLoading.value = true;
    previewError.value = "";
    previewManifest.value = null;
    previewMigrations.value = [];
    try {
        const res = await fetch("/api/plugins/preview", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ image: ref }),
        });
        if (!res.ok) throw new Error(`Preview failed (${res.status})`);
        const json = await res.json();
        previewManifest.value = json.data?.manifest || null;
        previewMigrations.value = json.data?.migrations || [];
        pluginSlug.value = json.data?.slug || selectedRepo.value || "";

        if (previewManifest.value?.settings_schema?.properties) {
            const init: Record<string, any> = {};
            for (const [k, p] of Object.entries(
                previewManifest.value.settings_schema.properties,
            )) {
                init[k] = (p as JsonSchemaProperty).default ?? "";
            }
            configSettings.value = init;
        }
    } catch (e) {
        previewError.value =
            e instanceof Error ? e.message : "Failed to load preview";
        toast.show(previewError.value, "error");
    } finally {
        previewLoading.value = false;
    }
}

async function startInstall() {
    const ref = getImageRef();
    const slug = selectedRepo.value || "";
    pluginSlug.value = slug;
    currentStep.value = 4;
    installPhase.value = "pulling";
    installError.value = "";

    try {
        const body: Record<string, any> = {
            slug,
            version: selectedTag.value,
            image: ref,
            registry_id: Number(selectedRegistryId.value),
            env: {},
            start_container: installMode.value === "install-and-start",
        };

        if (Object.keys(configSettings.value).length) {
            body.settings = configSettings.value;
        }
        const scopes = manifestScopes.value.map((s) => s.name);
        if (grantRootAccess.value) {
            scopes.push("rootaccess.all");
        }
        if (scopes.length) {
            body.granted_scopes = scopes;
        }

        const res = await fetch("/api/plugins/deploy", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
        });

        if (!res.ok) throw new Error(`Deploy failed (${res.status})`);

        installPhase.value = "done";
        await store.fetchPlugins();
        toast.show(`Plugin "${slug}" installed`, "success");
    } catch (e) {
        installPhase.value = "error";
        installError.value = e instanceof Error ? e.message : "Unknown error";
        toast.show(`Installation failed: ${installError.value}`, "error");
    }
}

function retryInstall() {
    installPhase.value = "pulling";
    installError.value = "";
    startInstall();
}

function viewPlugin() {
    if (pluginSlug.value) {
        router.push(`/plugins/${encodeURIComponent(pluginSlug.value)}`);
    }
}

function cancel() {
    router.push("/plugins");
}

function methodBadgeClass(m: string): string {
    const map: Record<string, string> = {
        GET: "text-green-600 font-mono",
        POST: "text-blue-600 font-mono",
        PUT: "text-orange-600 font-mono",
        PATCH: "text-purple-600 font-mono",
        DELETE: "text-red-600 font-mono",
    };
    return map[m.toUpperCase()] || "text-gray-600 font-mono";
}

onMounted(async () => {
    await fetchRegistries();
    const [firstRegistry] = registries.value;

    if (firstRegistry) {
        selectedRegistryId.value = firstRegistry.id + "";
        onRegistryChange();
    }
});
</script>

<template>
    <div class="p-6 container mx-auto">
        <router-link
            to="/plugins"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
            >← Back to Plugins</router-link
        >

        <h1 class="text-2xl font-bold mb-6">Add New Plugin</h1>

        <!-- Steps indicator -->
        <div
            class="flex gap-0 mb-8 border border-gray-200 rounded-lg overflow-hidden"
        >
            <div
                v-for="(label, idx) in steps"
                :key="idx"
                class="flex-1 text-center py-2.5 text-sm font-medium transition-colors cursor-pointer"
                :class="stepClass(idx)"
                @click="currentStep = idx"
            >
                <span class="hidden sm:inline">{{ idx + 1 }}. {{ label }}</span>
                <span class="sm:hidden">{{ idx + 1 }}</span>
            </div>
        </div>

        <!-- Step 1: Select Registry & Image -->
        <div v-show="currentStep === 0" class="space-y-4">
            <h2 class="text-lg font-semibold">Select Registry</h2>
            <Select
                v-model="selectedRegistryId"
                @change="onRegistryChange"
                :options="registries"
                option-label="name"
                :option-value="(r: any) => String(r.id)"
                class="w-full"
                placeholder="-- Select a registry --"
            />
            <p v-if="registries.length === 0" class="text-sm text-gray-500">
                No registries configured
            </p>

            <div v-if="selectedRegistryId">
                <h2 class="text-lg font-semibold mt-6 mb-3">Select Plugin</h2>
                <div
                    v-if="imagesLoading"
                    class="text-center py-8 text-gray-500"
                >
                    <i class="pi pi-spin pi-spinner text-2xl mb-2"></i>
                    <p>Loading images...</p>
                </div>
                <div
                    v-else-if="groupedRepos.length === 0"
                    class="text-center py-8 text-gray-500 border border-dashed border-gray-300 rounded-lg"
                >
                    No images found in this registry
                </div>
                <div
                    v-else
                    class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
                >
                    <div
                        v-for="repo in groupedRepos"
                        :key="repo.name"
                        class="border rounded-lg p-4 cursor-pointer transition-all hover:shadow-md hover:border-blue-300"
                        :class="
                            selectedRepo === repo.name
                                ? 'border-blue-500 bg-blue-50 ring-2 ring-blue-200'
                                : 'border-gray-200 bg-white'
                        "
                        @click="selectRepo(repo.name)"
                    >
                        <div class="flex items-start justify-between">
                            <div>
                                <h3 class="font-medium text-gray-900">
                                    {{ repo.name }}
                                </h3>
                                <p class="text-xs text-gray-500 mt-1">
                                    {{ repo.tagCount }} tag{{
                                        repo.tagCount !== 1 ? "s" : ""
                                    }}
                                </p>
                            </div>
                            <span
                                class="text-xs bg-gray-100 text-gray-600 px-2 py-1 rounded-full"
                                >{{ repo.latestTag }}
                            </span>
                        </div>
                    </div>
                </div>
            </div>

            <div class="flex justify-end pt-4 border-t border-gray-200">
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="cancel"
                    class="mr-2"
                />
                <Button
                    label="Next"
                    severity="primary"
                    :disabled="!selectedRepo"
                    @click="currentStep = 1"
                />
            </div>
        </div>

        <!-- Step 2: Select Version -->
        <div v-show="currentStep === 1" class="space-y-4">
            <h2 class="text-lg font-semibold">
                Select Version for
                <span class="text-blue-600">{{ selectedRepo }}</span>
            </h2>

            <div
                v-if="versions.length === 0"
                class="text-center py-8 text-gray-500 border border-dashed border-gray-300 rounded-lg"
            >
                No versions available
            </div>
            <div
                v-else
                class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3"
            >
                <div
                    v-for="v in versions"
                    :key="v.tag"
                    class="border rounded-lg p-3 cursor-pointer transition-all hover:shadow-md hover:border-blue-300 text-center"
                    :class="
                        selectedTag === v.tag
                            ? 'border-blue-500 bg-blue-50 ring-2 ring-blue-200'
                            : 'border-gray-200 bg-white'
                    "
                    @click="selectedTag = v.tag"
                >
                    <span class="font-mono font-medium text-sm">{{
                        v.tag
                    }}</span>
                </div>
            </div>

            <div class="flex justify-between pt-4 border-t border-gray-200">
                <Button
                    label="Back"
                    severity="secondary"
                    outlined
                    @click="currentStep = 0"
                />
                <Button
                    label="Next: Preview"
                    severity="primary"
                    :disabled="!selectedTag"
                    @click="goToPreview"
                />
            </div>
        </div>

        <!-- Step 3: Preview -->
        <div v-show="currentStep === 2" class="space-y-4">
            <div v-if="previewLoading" class="text-center py-8 text-gray-500">
                <i class="pi pi-spin pi-spinner text-2xl mb-2"></i>
                <p>Loading manifest...</p>
            </div>
            <div
                v-else-if="previewError"
                class="bg-red-50 border border-red-200 rounded-md p-4 text-sm text-red-700"
            >
                {{ previewError }}
            </div>
            <template v-else>
                <div
                    v-if="!previewManifest"
                    class="bg-yellow-50 border border-yellow-200 rounded-md p-4 text-sm text-yellow-700"
                >
                    No manifest.json found in this image. We do not advice you
                    to install this plugin.
                </div>

                <div v-if="previewManifest" class="space-y-2">
                    <div
                        class="bg-gray-50 rounded-lg p-4 border border-gray-200"
                    >
                        <div
                            class="flex place-content-between items-center gap-x-6 gap-y-2 text-sm"
                        >
                            <h2>
                                Plugin:
                                <span class="text-blue-600"
                                    >{{ selectedRepo }}:{{ selectedTag }}</span
                                >
                            </h2>
                            <div>
                                <span class="text-gray-500">Name:</span>
                                {{ previewManifest.name || "-" }}
                            </div>
                            <div
                                v-if="previewManifest.description"
                                class="col-span-2"
                            >
                                <span class="text-gray-500">Description:</span>
                                {{ previewManifest.description }}
                            </div>
                            <div>
                                <span class="text-gray-500">Type:</span>
                                {{ previewManifest.plugin_type || "dynamic" }}
                            </div>
                        </div>
                    </div>

                    <!-- Required Scopes -->
                    <div
                        v-if="previewManifest.scopes?.length"
                        class="bg-amber-50 rounded-lg p-4 border border-amber-200"
                    >
                        <h3 class="text-sm font-semibold text-amber-800 mb-2">
                            Required Scopes ({{
                                previewManifest.scopes.length
                            }})
                        </h3>
                        <p class="text-xs text-amber-700 mb-3">
                            Installing this plugin will grant the following
                            permissions:
                        </p>
                        <div class="gap-2 grid md:grid-cols-2">
                            <div
                                v-for="s in previewManifest.scopes"
                                :key="s.name"
                                class="flex items-center gap-3 p-2.5 bg-white rounded border border-amber-100"
                            >
                                <i class="pi pi-shield text-amber-500"></i>
                                <div>
                                    <span
                                        class="font-mono text-sm font-medium text-amber-900"
                                        >{{ s.name }}</span
                                    >
                                    <p
                                        v-if="s.description"
                                        class="text-xs text-amber-600"
                                    >
                                        {{ s.description }}
                                    </p>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Optional: Root Access -->
                    <div
                        class="bg-gray-50 rounded-lg p-4 border border-gray-200"
                    >
                        <h3 class="text-sm font-semibold text-gray-700 mb-2">
                            Optional: Full Access
                        </h3>
                        <label class="flex items-center gap-3 cursor-pointer">
                            <Checkbox
                                v-model="grantRootAccess"
                                :binary="true"
                            />
                            <div>
                                <span class="font-mono text-sm font-medium"
                                    >rootaccess.all</span
                                >
                                <p class="text-xs text-gray-500">
                                    Grants every permission — the plugin can
                                    access all APIs without individual scope
                                    restrictions. Use with caution.
                                </p>
                            </div>
                        </label>
                    </div>

                    <!-- // TODO show all frontend components -->

                    <Accordion value="0">
                        <AccordionPanel value="0">
                            <AccordionHeader>
                                Migrations ({{ previewMigrations.length }})
                            </AccordionHeader>
                            <AccordionContent>
                                <div
                                    v-for="m in previewMigrations"
                                    :key="m"
                                    class="flex items-center gap-2 text-sm font-mono text-gray-700 bg-white rounded px-3 py-1.5 border border-gray-100"
                                >
                                    <i
                                        class="pi pi-database text-gray-400 text-xs"
                                    ></i>
                                    <span>{{ m }}</span>
                                </div>
                            </AccordionContent>
                        </AccordionPanel>
                        <AccordionPanel value="1">
                            <AccordionHeader> Settings </AccordionHeader>
                            <AccordionContent>
                                <div class="space-y-2">
                                    <div
                                        v-for="(prop, key) in previewManifest
                                            .settings_schema?.properties"
                                        :key="key + ''"
                                        class="text-sm"
                                    >
                                        <span
                                            class="font-medium text-gray-700"
                                            >{{ key }}</span
                                        >
                                        <span
                                            v-if="prop.type"
                                            class="text-xs text-gray-400 ml-1"
                                            >({{ prop.type }})</span
                                        >
                                        <span
                                            v-if="prop.default"
                                            class="text-xs text-gray-400 ml-1"
                                            >default: {{ prop.default }}</span
                                        >
                                    </div>
                                </div>
                            </AccordionContent>
                        </AccordionPanel>
                        <AccordionPanel value="2">
                            <AccordionHeader>
                                Pages ({{
                                    previewManifest.pages?.length
                                }})</AccordionHeader
                            >
                            <AccordionContent>
                                <div class="grid grid-cols-2 gap-2">
                                    <div
                                        v-for="page in previewManifest.pages"
                                        :key="page.path"
                                        class="flex items-center gap-2 text-sm bg-white rounded p-2 border border-gray-100"
                                    >
                                        <span class="text-lg">{{
                                            page.icon || "📄"
                                        }}</span>
                                        <div>
                                            <p class="font-medium">
                                                {{ page.label }}
                                            </p>
                                            <p class="text-xs text-gray-400">
                                                {{ page.path }}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            </AccordionContent>
                        </AccordionPanel>
                        <AccordionPanel value="3">
                            <AccordionHeader>
                                Endpoints ({{
                                    previewManifest.endpoints.length
                                }})
                            </AccordionHeader>
                            <AccordionContent>
                                <div class="flex flex-wrap gap-2">
                                    <span
                                        v-for="ep in previewManifest.endpoints"
                                        :key="ep.method + ep.path"
                                        class="inline-flex items-center gap-1 text-xs bg-white rounded border border-gray-200 px-2 py-1"
                                    >
                                        <span
                                            :class="methodBadgeClass(ep.method)"
                                            >{{ ep.method }}</span
                                        >
                                        <span class="text-gray-600">{{
                                            ep.path
                                        }}</span>
                                    </span>
                                </div>
                            </AccordionContent>
                        </AccordionPanel>
                    </Accordion>
                </div>
            </template>

            <div class="flex justify-between pt-4 border-t border-gray-200">
                <Button
                    label="Back"
                    severity="secondary"
                    outlined
                    @click="currentStep = 1"
                />
                <Button
                    label="Next: Configure"
                    severity="primary"
                    :disabled="previewLoading"
                    @click="currentStep = 3"
                />
            </div>
        </div>

        <!-- Step 4: Configure -->
        <div v-show="currentStep === 3" class="space-y-4">
            <h2 class="text-lg font-semibold">
                Configure
                <span class="text-blue-600"
                    >{{ selectedRepo }}:{{ selectedTag }}</span
                >
            </h2>

            <div
                v-if="settingsSchema"
                class="bg-white rounded-lg border border-gray-200 p-4"
            >
                <h3 class="text-sm font-semibold text-gray-700 mb-3">
                    Environment Settings
                </h3>
                <DynamicFormField
                    v-for="(prop, key) in settingsSchema.properties"
                    :key="key"
                    :field-name="String(key)"
                    :property="prop"
                    :model-value="configSettings[String(key)]"
                    @update:model-value="configSettings[String(key)] = $event"
                />
            </div>

            <div class="bg-white rounded-lg border border-gray-200 p-4">
                <h3 class="text-sm font-semibold text-gray-700 mb-2">
                    Install Mode
                </h3>
                <div class="flex gap-4">
                    <label class="flex items-center gap-2 cursor-pointer">
                        <RadioButton
                            v-model="installMode"
                            input-id="install-only"
                            value="install"
                        />
                        <span class="text-sm"
                            >Register only (no container)</span
                        >
                    </label>
                    <label class="flex items-center gap-2 cursor-pointer">
                        <RadioButton
                            v-model="installMode"
                            input-id="install-and-start"
                            value="install-and-start"
                        />
                        <span class="text-sm">Install &amp; Start</span>
                    </label>
                </div>
            </div>

            <div class="flex justify-between pt-4 border-t border-gray-200">
                <Button
                    label="Back"
                    severity="secondary"
                    outlined
                    @click="currentStep = 2"
                />
                <Button
                    label="Install"
                    severity="primary"
                    @click="startInstall"
                />
            </div>
        </div>

        <!-- Step 5: Install Progress -->
        <div v-show="currentStep === 4" class="py-12 text-center space-y-4">
            <div v-if="installPhase === 'error'" class="text-red-500">
                <i class="pi pi-times-circle text-4xl mb-2"></i>
                <p class="font-medium text-lg">Installation Failed</p>
                <p class="text-sm mt-1 text-gray-600">{{ installError }}</p>
                <div class="flex justify-center gap-2 mt-6">
                    <Button
                        label="Retry"
                        severity="primary"
                        @click="retryInstall"
                    />
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="cancel"
                    />
                </div>
            </div>
            <div v-else-if="installPhase === 'done'" class="text-green-500">
                <i class="pi pi-check-circle text-4xl mb-2"></i>
                <p class="font-medium text-lg">Plugin Installed!</p>
                <p class="text-sm text-gray-500 mt-1">
                    {{ pluginSlug }} ({{ selectedTag }})
                </p>
                <div class="flex justify-center gap-2 mt-6">
                    <Button
                        label="View Plugin"
                        severity="primary"
                        @click="viewPlugin"
                    />
                    <Button
                        label="Back to List"
                        severity="secondary"
                        outlined
                        @click="cancel"
                    />
                </div>
            </div>
            <div v-else class="text-gray-600">
                <i class="pi pi-spin pi-spinner text-3xl mb-3"></i>
                <p class="font-medium">{{ installPhaseLabel }}</p>
                <div
                    class="max-w-xs mx-auto mt-4 bg-gray-200 rounded-full h-1.5"
                >
                    <div
                        class="bg-blue-500 h-1.5 rounded-full transition-all duration-500"
                        :style="{ width: installProgress + '%' }"
                    ></div>
                </div>
            </div>
        </div>
    </div>
</template>

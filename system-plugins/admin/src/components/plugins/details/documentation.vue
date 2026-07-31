<script setup lang="ts">
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { nextTick, onMounted, ref } from "vue";
import { computed } from "vue";
import { useRoute } from "vue-router";
import DocsViewer from "./DocsViewer.vue";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute(),
    store = usePluginsStore();

const docs = ref<Array<{ path: string; size: number }>>([]),
    selectedDoc = ref<string | null>(null),
    docContent = ref(""),
    docsLoading = ref(false),
    docsError = ref<string | null>(null),
    expandedDocDirs = ref<Set<string>>(new Set());

interface DocTreeItem {
    path: string;
    name: string;
    isDir: boolean;
    expanded: boolean;
    children?: DocTreeItem[];
}

const docTree = computed<DocTreeItem[]>(() => {
    const root: DocTreeItem[] = [];
    const dirMap = new Map<string, DocTreeItem>();

    for (const doc of docs.value) {
        const parts = doc.path.split("/");
        if (parts.length === 1) {
            root.push({
                path: doc.path,
                name: doc.path,
                isDir: false,
                expanded: false,
            });
        } else {
            const dirName = parts[0];
            const fileName = parts[parts.length - 1];
            const dirPath = dirName + "/";

            let dir = dirMap.get(dirName);
            if (!dir) {
                dir = {
                    path: dirPath,
                    name: dirName,
                    isDir: true,
                    expanded: expandedDocDirs.value.has(dirPath),
                    children: [],
                };
                dirMap.set(dirName, dir);
                root.push(dir);
            }
            dir.children!.push({
                path: doc.path,
                name: fileName,
                isDir: false,
                expanded: false,
            });
        }
    }
    return root;
});

function toggleDocDir(path: string) {
    if (expandedDocDirs.value.has(path)) {
        expandedDocDirs.value.delete(path);
    } else {
        expandedDocDirs.value.add(path);
    }
}

async function loadDocs() {
    await withAsyncHandlingVoid(
        docsLoading,
        docsError,
        async () => {
            const res = await store.fetchPluginDocs(
                route.params.name as string,
            );

            docs.value = res.docs.map((d) => ({ path: d.path, size: d.size }));
            if (docs.value.length > 0 && !selectedDoc.value) {
                await selectDoc(docs.value[0].path);
            }
        },
        "Failed to load documentation",
    );
}

async function selectDoc(docPath: string) {
    selectedDoc.value = docPath;
    docContent.value = "";
    docsLoading.value = true;
    docsError.value = null;
    await nextTick();
    try {
        const name = route.params.name as string;
        docContent.value = await store.fetchDocContent(name, docPath);
    } catch (e) {
        docsError.value =
            e instanceof Error ? e.message : "Failed to load document content";
    } finally {
        docsLoading.value = false;
    }
}

onMounted(() => {
    loadDocs();
});
</script>

<template>
    <div
        v-if="docs && docs.length > 0"
        class="flex flex-col-reverse lg:flex-row"
    >
        <!-- Main: doc viewer -->
        <div class="flex-1 min-w-0">
            <div v-if="docsLoading" class="text-gray-500">
                Loading documentation...
            </div>
            <div
                v-else-if="docsError"
                class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
            >
                {{ docsError }}
            </div>
            <DocsViewer
                v-else
                :content="docContent"
                :loading="docsLoading"
                :error="docsError"
                @navigate="selectDoc"
            />
        </div>
        <!-- Sidebar: doc list on right -->
        <div
            class="w-full lg:w-56 shrink-0 lg:border-l lg:border-gray-200 lg:pl-4 lg:border-b-0 border-b border-gray-200 pt-4 lg:pt-0"
        >
            <h4 class="text-sm font-medium text-gray-700 mb-2">Files</h4>
            <ul class="space-y-1">
                <li v-for="doc in docTree" :key="doc.path">
                    <Button
                        v-if="doc.isDir"
                        text
                        severity="secondary"
                        class="w-full text-left!"
                        @click="toggleDocDir(doc.path)"
                    >
                        <div class="mr-auto">
                            {{ (doc.expanded ? "📂" : "📁") + " " + doc.name }}
                        </div>
                    </Button>
                    <Button
                        v-else
                        text
                        severity="secondary"
                        class="w-full text-left!"
                        :class="{
                            'bg-blue-100 text-blue-700 font-medium':
                                selectedDoc === doc.path,
                            'text-gray-600 hover:bg-gray-100':
                                selectedDoc !== doc.path,
                        }"
                        @click="selectDoc(doc.path)"
                    >
                        <div class="mr-auto">
                            {{ "📄 " + doc.name }}
                        </div>
                    </Button>
                    <ul
                        v-if="doc.isDir && doc.expanded"
                        class="ml-4 mt-1 space-y-1"
                    >
                        <li v-for="child in doc.children" :key="child.path">
                            <Button
                                text
                                severity="secondary"
                                class="w-full text-left"
                                :class="{
                                    'bg-blue-100 text-blue-700 font-medium':
                                        selectedDoc === child.path,
                                    'text-gray-600 hover:bg-gray-100':
                                        selectedDoc !== child.path,
                                }"
                                @click="selectDoc(child.path)"
                            >
                                <div class="mr-auto">
                                    {{ "📄 " + child.name }}
                                </div>
                            </Button>
                        </li>
                    </ul>
                </li>
            </ul>
        </div>
    </div>
    <div v-else class="text-gray-400 italic">
        No documentation available for this plugin
    </div>
</template>

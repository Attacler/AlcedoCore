<script setup lang="ts">
import { ref, computed } from "vue";
import { usePluginsStore } from "@/stores/plugins";
import { useCollectionsStore } from "@/stores/collections";
import { useMenuStore } from "@/stores/menuStore";
import { Menu } from "primevue";

const store = useMenuStore(),
    pluginsStore = usePluginsStore(),
    collectionsStore = useCollectionsStore();

const props = defineProps<{
    sectionId: string;
}>();

const quickAddMenu = ref<InstanceType<typeof Menu> | null>(null);

const quickAddMenuItems = computed(() => {
    const { sectionId } = props;
    if (!sectionId) return [];

    const collections = collectionsStore.collections.map((c) => ({
        label: c.display_name || c.name,
        icon: "pi pi-database",
        command: () => {
            store.addItem(sectionId!);
            const section = store.editSections.find((s) => s.id === sectionId);
            if (section && section.items.length > 0) {
                const lastItem = section.items[section.items.length - 1];
                store.updateItem(lastItem.id, {
                    label: c.display_name || c.name,
                    icon: "database",
                    linkType: "collection",
                    route: `/collections/${c.name}/data`,
                });
            }
        },
    }));

    const pluginPages = pluginsStore.enabledPlugins.flatMap((plugin) => {
        const pages = pluginsStore.getCachedPluginPages(plugin.name) || [];
        return pages.map((p) => ({
            label: `${plugin.displayName || plugin.name}: ${p.label}`,
            icon: "pi pi-extension",
            command: () => {
                store.addItem(sectionId!);
                const section = store.editSections.find(
                    (s) => s.id === sectionId,
                );
                if (section && section.items.length > 0) {
                    const lastItem = section.items[section.items.length - 1];
                    store.updateItem(lastItem.id, {
                        label: p.label,
                        icon: "extension",
                        linkType: "plugin",
                        route: `/p/${encodeURIComponent(plugin.name)}${p.path}`,
                    });
                }
            },
        }));
    });

    return [
        {
            label: "Collections",
            items:
                collections.length > 0
                    ? collections
                    : [{ label: "No collections", disabled: true }],
        },
        {
            label: "Plugin Pages",
            items:
                pluginPages.length > 0
                    ? pluginPages
                    : [{ label: "No plugin pages", disabled: true }],
        },
    ];
});

function toggleQuickAdd(event: Event) {
    quickAddMenu.value?.toggle(event);
}
</script>

<template>
    <Button
        icon="pi pi-bolt"
        text
        severity="secondary"
        rounded
        @click.stop="toggleQuickAdd($event)"
        title="Quick add"
    />
    <Menu ref="quickAddMenu" :model="quickAddMenuItems" :popup="true" />
</template>

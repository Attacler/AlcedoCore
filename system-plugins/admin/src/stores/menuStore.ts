import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { Menu, MenuSection, MenuItem } from "../types/menu";
import { createEmptySection, createEmptyItem } from "../types/menu";
import { useExtensionRegistryStore } from "./extensionRegistry";
import { useAlcedoClient } from "../composables/useAlcedoClient";

function deepClone<T>(data: T): T {
    return JSON.parse(JSON.stringify(data));
}

function findItemInMenu(items: MenuItem[], id: string): MenuItem | null {
    for (const item of items) {
        if (item.id === id) return item;
        if (item.children) {
            const found = findItemInMenu(item.children, id);
            if (found) return found;
        }
    }
    return null;
}

export const useMenuStore = defineStore("menu", () => {
    const { client } = useAlcedoClient();

    // ── State ──
    const menus = ref<Menu[]>([]);
    const activeMenuId = ref<string | null>(null);
    const activeEditMenuId = ref<string | null>(null);
    const loading = ref(false);
    const error = ref<string | null>(null);
    const selectedItemId = ref<string | null>(null);
    const saving = ref(false);

    const editSections = ref<MenuSection[]>([]);
    const editMenuName = ref("");
    const editMenuIcon = ref("menu");
    const originalEditSections = ref<MenuSection[]>([]);

    // ── Computed ──

    const activeMenu = computed<Menu | null>(() => {
        if (!activeMenuId.value) return null;
        return menus.value.find((m) => m.id === activeMenuId.value) || null;
    });

    const activeEditMenu = computed<Menu | null>(() => {
        if (!activeEditMenuId.value) return null;
        return menus.value.find((m) => m.id === activeEditMenuId.value) || null;
    });

    const isDirty = computed(() => {
        return (
            JSON.stringify(editSections.value) !==
            JSON.stringify(originalEditSections.value)
        );
    });

    const selectedItem = computed<MenuItem | null>(() => {
        if (!selectedItemId.value) return null;
        for (const sec of editSections.value) {
            const found = findItemInMenu(sec.items, selectedItemId.value);
            if (found) return found;
        }
        return null;
    });

    const mergedSections = computed<MenuSection[]>(() => {
        const active = activeMenu.value;
        if (!active) return [];

        return deepClone(active.sections);
    });

    // ── Actions ──

    async function loadMyMenus() {
        loading.value = true;
        error.value = null;
        try {
            const res = await fetch("/api/menus/my", {
                credentials: "include",
            });
            if (!res.ok) throw new Error("Failed to load menus");
            const json = await res.json();
            menus.value = json.data || [];

            const lastId = localStorage.getItem("activeMenuId");
            if (lastId && menus.value.some((m) => m.id === lastId)) {
                activeMenuId.value = lastId;
            } else if (menus.value.length > 0) {
                activeMenuId.value = menus.value[0].id;
            } else {
                activeMenuId.value = null;
            }
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to load menus";
            menus.value = [];
            activeMenuId.value = null;
        } finally {
            loading.value = false;
        }
    }

    function setActiveMenu(id: string) {
        activeMenuId.value = id;
        localStorage.setItem("activeMenuId", id);
    }

    function loadEditMenu(menuId: string) {
        const menu = menus.value.find((m) => m.id === menuId);
        if (menu) {
            activeEditMenuId.value = menuId;
            editSections.value = deepClone(menu.sections);
            editMenuName.value = menu.name;
            editMenuIcon.value = menu.icon;
            originalEditSections.value = deepClone(menu.sections);
            selectedItemId.value = null;
        }
    }

    async function saveEditMenu(menuId?: string): Promise<boolean> {
        const id = menuId || activeEditMenuId.value;
        if (!id) return false;
        saving.value = true;
        try {
            const json = await client.request("put", `menus/${id}`, {
                json: {
                    name: editMenuName.value,
                    icon: editMenuIcon.value,
                    sections: editSections.value,
                },
            });
            const idx = menus.value.findIndex((m) => m.id === id);
            if (idx !== -1 && json.data) {
                menus.value[idx] = json.data;
            }
            originalEditSections.value = deepClone(editSections.value);
            return true;
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to save menu";
            throw e;
        } finally {
            saving.value = false;
        }
    }

    function cancelEditChanges() {
        editSections.value = deepClone(originalEditSections.value);
        selectedItemId.value = null;
    }

    function selectItem(id: string | null) {
        selectedItemId.value = id;
    }

    function addSection(label?: string) {
        editSections.value.push(createEmptySection(label));
    }

    function deleteSection(sectionId: string) {
        const idx = editSections.value.findIndex((s) => s.id === sectionId);
        if (idx !== -1) {
            editSections.value.splice(idx, 1);
        }
    }

    function updateSection(sectionId: string, patch: Partial<MenuSection>) {
        const section = editSections.value.find((s) => s.id === sectionId);
        if (section) Object.assign(section, patch);
    }

    function addItem(sectionId: string, parentItemId?: string) {
        const section = editSections.value.find((s) => s.id === sectionId);
        if (!section) return;
        const newItem = createEmptyItem();
        if (parentItemId) {
            const parent = findItemInMenu(section.items, parentItemId);
            if (parent) {
                if (!parent.children) parent.children = [];
                parent.children.push(newItem);
            }
        } else {
            section.items.push(newItem);
        }
    }

    function deleteItem(sectionId: string, itemId: string) {
        const section = editSections.value.find((s) => s.id === sectionId);
        if (!section) return;
        const idx = section.items.findIndex((i) => i.id === itemId);
        if (idx !== -1) {
            section.items.splice(idx, 1);
            if (selectedItemId.value === itemId) selectedItemId.value = null;
            return;
        }
        for (const item of section.items) {
            if (item.children) {
                const childIdx = item.children.findIndex(
                    (c) => c.id === itemId,
                );
                if (childIdx !== -1) {
                    item.children.splice(childIdx, 1);
                    if (selectedItemId.value === itemId)
                        selectedItemId.value = null;
                    return;
                }
            }
        }
    }

    function updateItem(itemId: string, patch: Partial<MenuItem>) {
        for (const section of editSections.value) {
            for (const item of section.items) {
                if (item.id === itemId) {
                    Object.assign(item, patch);
                    return;
                }
                if (item.children) {
                    const child = item.children.find((c) => c.id === itemId);
                    if (child) {
                        Object.assign(child, patch);
                        return;
                    }
                }
            }
        }
    }

    function toggleVisibility(id: string) {
        const section = editSections.value.find((s) => s.id === id);
        if (section) {
            section.visible = !section.visible;
            return;
        }
        for (const s of editSections.value) {
            const item = findItemInMenu(s.items, id);
            if (item) {
                item.visible = !item.visible;
                return;
            }
        }
    }

    return {
        menus,
        activeMenuId,
        activeEditMenuId,
        loading,
        error,
        selectedItemId,
        saving,
        editSections,
        editMenuName,
        editMenuIcon,
        originalEditSections,
        activeMenu,
        isDirty,
        selectedItem,
        mergedSections,
        loadMyMenus,
        setActiveMenu,
        loadEditMenu,
        saveEditMenu,
        cancelEditChanges,
        selectItem,
        addSection,
        deleteSection,
        updateSection,
        addItem,
        deleteItem,
        updateItem,
        toggleVisibility,
        activeEditMenu,
    };
});

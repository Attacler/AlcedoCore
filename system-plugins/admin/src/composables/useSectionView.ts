import { type Component } from "vue";
import TableView from "@/views/TableView.vue";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import type { FilterCondition } from "@/types/filters";

export function useSectionView() {
    const extensionRegistry = useExtensionRegistryStore();

    /** Resolve a section's view component: builtins, plugin manifest views
     *  ("plugin:slug:name"), runtime-registered view types — fallback table. */
    function sectionViewComponent(viewType: string): Component {
        return extensionRegistry.getView(viewType) ?? TableView;
    }

    return { sectionViewComponent };
}

/** Combine a section's configured filter with the parent-FK rule. */
export function buildSectionFilter(
    base: FilterCondition | null | undefined,
    fkRule: FilterCondition | null,
): FilterCondition | null {
    const conditions = [base, fkRule].filter((c): c is FilterCondition => !!c);
    if (conditions.length === 0) return null;
    if (conditions.length === 1) return conditions[0];
    return { operator: "and", conditions };
}

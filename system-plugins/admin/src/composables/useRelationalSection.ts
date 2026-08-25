import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useCollectionsStore } from "@/stores/collections";
import { getSectionChildCollectionName } from "@/composables/useSectionLayout";
import { buildSectionFilter } from "@/composables/useSectionView";
import { toShortForm } from "@/types/filters";
import type { FieldDefinition } from "@/stores/collections";
import type { FilterCondition } from "@/types/filters";

/** Resolve the child collection name for a namespaced relational section. */
export function resolveChildCollection(section: any): string {
    return getSectionChildCollectionName(section?.relation_field);
}

/** Find the relationship field on the child collection that points back to the parent. */
export function findParentFKField(
    sectionFields: FieldDefinition[],
    parentCollectionName: string,
): FieldDefinition | null {
    return (
        sectionFields.find(
            (f: any) =>
                f.type === "relationship" &&
                f.related_collection === parentCollectionName,
        ) || null
    );
}

/** Combine a section's configured filter with the parent-FK rule. */
export function buildSectionFilterCondition(
    section: any,
    sectionFields: FieldDefinition[],
    parentCollectionName: string,
    parentItemId: string | null | undefined,
): FilterCondition | null {
    const fkField = findParentFKField(sectionFields, parentCollectionName);
    const fkRule =
        fkField && parentItemId
            ? {
                  field: fkField.name,
                  operator: "eq" as const,
                  value: parentItemId,
              }
            : null;
    return buildSectionFilter(section?.default_filter?.filter, fkRule);
}

/** Load the fields of a section's child collection. */
export async function loadSectionFields(
    childCollectionName: string,
): Promise<FieldDefinition[]> {
    if (!childCollectionName) return [];
    try {
        const coll =
            await useCollectionsStore().getCollection(childCollectionName);
        return coll.fields || [];
    } catch (e) {
        console.warn("[RelationalSection] Failed to load section fields", e);
        return [];
    }
}

// TODO: there are multiple places where logic like this exists, we need to make one central "permissions" store for this
/** Whether the caller may create records in the child collection. */
export async function fetchCreatePermission(
    childCollectionName: string,
): Promise<boolean> {
    if (!childCollectionName) return false;
    try {
        const policy =
            (await useAlcedoClient().client.collections.getCreatePolicy(
                childCollectionName,
            )) as any;
        return policy?.$permissions?.create !== false;
    } catch {
        return true;
    }
}

/** Load child items for a relational section, filtered to the parent. */
export async function loadSectionData(opts: {
    childCollectionName: string;
    parentItemId: string | null | undefined;
    section: any;
    sectionFields: FieldDefinition[];
    parentCollectionName: string;
}): Promise<{ items: any[]; total: number }> {
    const {
        childCollectionName,
        parentItemId,
        section,
        sectionFields,
        parentCollectionName,
    } = opts;
    if (!childCollectionName || !parentItemId) {
        return { items: [], total: 0 };
    }
    const queryParams = new URLSearchParams();
    queryParams.set("limit", String(section?.item_limit || 25));
    queryParams.set("offset", "0");

    const filterCondition = buildSectionFilterCondition(
        section,
        sectionFields,
        parentCollectionName,
        parentItemId,
    );
    if (filterCondition) {
        queryParams.set("filter", JSON.stringify(toShortForm(filterCondition)));
    }

    const res = (await useAlcedoClient().client.items.list(
        childCollectionName,
        queryParams as unknown as Record<string, string>,
    )) as any;
    const data = res.data || res;
    const items = data.data || data.items || data || [];
    return { items, total: data.total || items.length };
}

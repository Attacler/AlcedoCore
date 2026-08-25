<script lang="ts" setup>
import { ref } from "vue";
import CollectionNameLabel from "../CollectionNameLabel.vue";
import { FieldDefinition, useCollectionsStore } from "@/stores/collections";
import { useToast } from "@/composables/useToast";
import { Drawer, InputText, Button } from "primevue";

const props = defineProps<{
    collectionMeta: any;
    collectionName: string;
    activeLayoutId: string;
    sections: any[];
}>();

const fields = defineModel<(FieldDefinition & { _key: string })[]>("fields", {
    required: true,
});

const store = useCollectionsStore(),
    toast = useToast();

const saving = ref(false);

let keyCounter = 0;
function nextKey(): string {
    return `f_${++keyCounter}_${Date.now()}`;
}

async function handleSave() {
    const validFields = fields.value.filter(
        (f) => f.name && /^[a-z][a-z0-9_]*$/.test(f.name),
    );
    if (validFields.length === 0) {
        toast.show("No valid fields to save", "error");
        return;
    }
    saving.value = true;
    try {
        const payload = validFields
            .filter((f) => !f.is_system)
            .map((f, i) => {
            const p: any = {
                name: f.name,
                display_name: f.display_name || null,
                type: f.type,
                required: f.required,
                unique: f.unique,
                default_value: f.default_value,
                display_type: f.display_type,
                input_component: f.input_component,
                display_component: f.display_component,
                ordinal_position: i + 1,
            };
            const a = f as any;

            if (a.related_collection) {
                p.related_collection = a.related_collection;
                p.relationship_type = a.relationship_type;
            }
            if (a.display_field) {
                p.display_field = a.display_field;
            }
            if (a.inline_parent_fields?.length > 0) {
                p.inline_parent_fields = a.inline_parent_fields;
            }
            if (
                a.options &&
                (Array.isArray(a.options) ? a.options.length > 0 : true)
            ) {
                p.options = a.options;
            }
            return p;
        });
        await store.updateCollection(props.collectionName, { fields: payload });

        // Persist section display_fields so newly added fields appear in their sections
        if (!props.activeLayoutId) {
            saving.value = false;
            return;
        }

        for (const section of props.sections) {
            if (section.id) {
                const sectionType = section.section_type || "field_group";
                const sectionPayload: any = {
                    name: section.name,
                    section_type: sectionType,
                    ordinal_position: section.ordinal_position,
                    default_filter: null,
                };
                if (sectionType === "field_group") {
                    sectionPayload.display_fields =
                        section.display_fields || [];
                    sectionPayload.relation_field = null;
                    sectionPayload.view_type = null;
                    sectionPayload.item_limit = null;
                    sectionPayload.default_filter =
                        (section._columns ?? 0) > 1
                            ? {
                                  _columns: section._columns,
                                  _field_columns:
                                      section._field_columns || {},
                              }
                            : null;
                } else {
                    sectionPayload.display_fields = null;
                    sectionPayload.relation_field =
                        section.relation_field || "";
                    sectionPayload.view_type =
                        section.view_type || "table";
                    sectionPayload.item_limit = section.item_limit || 25;
                    sectionPayload.default_filter =
                        section.default_filter || null;
                }
                await store
                    .updateLayoutSection(
                        props.collectionName,
                        props.activeLayoutId,
                        section.id,
                        sectionPayload,
                    )
                    .catch((e: any) =>
                        console.warn(
                            "[CollectionBuilder] Failed to persist section",
                            section.name,
                            e,
                        ),
                    );
            }
        }

        fields.value = validFields.map((f) => ({ ...f, _key: nextKey() }));
        toast.show("Collection saved successfully", "success");
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        saving.value = false;
    }
}
</script>

<template>
    <Button
        :label="saving ? 'Saving...' : 'Save Changes'"
        severity="primary"
        :disabled="saving"
        @click="handleSave"
    />
</template>

<script setup lang="ts">
import type { Component } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import Drawer from "primevue/drawer";

const props = defineProps<{
    settingsComponent: Component | null;
    collectionName: string;
    fields: FieldDefinition[];
    settings: Record<string, any>;
}>();

const emit = defineEmits<{
    "settings-change": [key: string, value: any];
}>();

const visible = defineModel<boolean>("visible");

function onSettingsChange(key: string, value: any) {
    emit("settings-change", key, value);
}
</script>

<template>
    <Drawer
        v-model:visible="visible"
        header="View Settings"
        position="right"
        class="w-full max-w-lg"
    >
        <component
            :is="settingsComponent"
            v-if="settingsComponent"
            :fields="fields"
            :settings="settings"
            :collection-name="collectionName"
            :on-change="onSettingsChange"
        />
    </Drawer>
</template>

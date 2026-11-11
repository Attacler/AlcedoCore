<script setup lang="ts">
import { computed, onMounted, provide } from "vue";
import { useRoute } from "vue-router";
import AppLayout from "@/components/AppLayout.vue";
import GlobalLayout from "@/components/GlobalLayout.vue";
import RelationalDrawer from "@/components/RelationalDrawer.vue";
import { registerSystemViewTypes } from "@/stores/plugins";
import FormFieldRenderer from "./components/FormFieldRenderer.vue";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import { useDisplayComponents } from "@/composables/useDisplayComponents";

const route = useRoute();

const isLoginPage = computed(() => route.name === "Login");
const isAppZone = computed(() => route.meta.appZone === true);

onMounted(() => {
    registerSystemViewTypes();
});

provide("FormFieldRenderer", FormFieldRenderer);
provide("FieldNameLabel", FieldNameLabel);

provide("useDisplayComponents", useDisplayComponents);
</script>

<template>
    <router-view v-if="isLoginPage" />
    <GlobalLayout v-else-if="!isAppZone" />
    <AppLayout v-else />
    <RelationalDrawer />
</template>

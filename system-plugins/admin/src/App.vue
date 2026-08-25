<script setup lang="ts">
import { provide, ref, watch } from "vue";
import { useRouter } from "vue-router";
import AppLayout from "@/components/AppLayout.vue";
import RelationalDrawer from "@/components/RelationalDrawer.vue";
import { usePluginsStore } from "@/stores/plugins";
import { useAuthStore } from "@/stores/authStore";
import FormFieldRenderer from "./components/FormFieldRenderer.vue";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import { useDisplayComponents } from "@/composables/useDisplayComponents";

const router = useRouter();
const isLoginPage = ref(false);

router.isReady().then(() => {
    isLoginPage.value = router.currentRoute.value.name === "Login";

    router.afterEach((to) => {
        isLoginPage.value = to.name === "Login";
    });
});

provide("FormFieldRenderer", FormFieldRenderer);
provide("FieldNameLabel", FieldNameLabel);

provide("useDisplayComponents", useDisplayComponents);
</script>

<template>
    <template v-if="isLoginPage">
        <router-view />
    </template>
    <template v-else>
        <AppLayout />
    </template>
    <RelationalDrawer />
</template>

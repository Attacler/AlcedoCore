<script setup>
import { useToast } from "@/composables/useToast";
import { useDevServerStore } from "@/stores/devServerStore";
import { Drawer, Message } from "primevue";
import { ref, watch } from "vue";

const devStore = useDevServerStore(),
    toast = useToast();

const visible = ref(false);

function attemptConnection() {
    devStore.connectionError = false;
    devStore.connect();
}

watch(
    () => visible.value,
    (newVal) => {
        if (newVal == true && !devStore.connected) {
            attemptConnection();
        }
    },
);

function disconnect() {
    visible.value = false;
    devStore.disconnect();
    devStore.enabled = false;
    toast.show("Devmode disabled!");
}
</script>

<template>
    <Drawer
        v-model:visible="visible"
        header="Development mode"
        position="right"
    >
        <div class="flex h-full w-full pt-2">
            <div v-if="devStore.connected" class="grow">
                <Message severity="success">Connected</Message>
                <table class="w-full">
                    <tr>
                        <th>Slug</th>
                        <td>{{ devStore.pluginManifest.pluginSlug }}</td>
                    </tr>
                    <tr>
                        <th>Version</th>
                        <td>{{ devStore.pluginManifest.version }}</td>
                    </tr>
                </table>
                <Button @click="disconnect" severity="warn" class="w-full">
                    Disconnect
                </Button>
            </div>
            <div class="flex flex-col item-center m-auto gap-4" v-else>
                <template
                    v-if="!devStore.connected && !devStore.connectionError"
                >
                    Connecting to 127.0.0.1:3003
                    <ProgressSpinner />
                </template>
                <template v-else-if="devStore.connectionError">
                    <Message class="text-center" severity="error">
                        Could not connect to the devserver, please start it.
                    </Message>
                    <Button @click="attemptConnection">Retry</Button>
                </template>
            </div>
        </div>
    </Drawer>
    <Button label="Enable development mode" @click="visible = true" />
</template>

<style scoped>
th,
td {
    text-align: left;
    padding: 5px 10px;
}
</style>

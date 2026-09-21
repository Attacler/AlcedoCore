<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import type { SessionInfo } from "@alcedocore/sdk";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useToast } from "@/composables/useToast";

const props = defineProps<{ userId?: string }>();
const emit = defineEmits<{ (e: "revoked-all"): void }>();

const { client } = useAlcedoClient();
const authStore = useAuthStore();
const toast = useToast();

const sessions = ref<SessionInfo[]>([]);
const loading = ref(false);
const revokingId = ref<string | null>(null);
const revokingAll = ref(false);

const targetId = computed(() => props.userId ?? authStore.user?.id ?? null);
const isSelf = computed(
    () => !props.userId || props.userId === authStore.user?.id,
);

function formatDate(value: string | null): string {
    if (!value) return "—";
    const date = new Date(
        /(Z|[+-]\d\d:\d\d)$/.test(value) ? value : `${value}Z`,
    );
    return isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function load() {
    if (!targetId.value) return;
    loading.value = true;
    try {
        sessions.value = isSelf.value
            ? await client.sessions.list()
            : await client.users.sessions(targetId.value);
    } catch (e: any) {
        toast.show(e?.message || "Failed to load sessions", "error");
    } finally {
        loading.value = false;
    }
}

onMounted(load);
watch(() => props.userId, load);

async function revoke(session: SessionInfo) {
    if (!targetId.value) return;
    if (session.current) {
        toast.show("This is your current session", "error");
        return;
    }
    revokingId.value = session.id;
    try {
        if (isSelf.value) {
            await client.sessions.revoke(session.id);
        } else {
            await client.users.revokeSession(targetId.value, session.id);
        }
        sessions.value = sessions.value.filter((s) => s.id !== session.id);
        toast.show("Session revoked", "success");
    } catch (e: any) {
        toast.show(e?.message || "Failed to revoke session", "error");
    } finally {
        revokingId.value = null;
    }
}

async function revokeAll() {
    if (!targetId.value) return;
    revokingAll.value = true;
    try {
        if (isSelf.value) {
            await client.sessions.revokeAll();
        } else {
            await client.users.revokeAllSessions(targetId.value);
        }
        sessions.value = [];
        toast.show("All sessions revoked", "success");
        if (isSelf.value) emit("revoked-all");
    } catch (e: any) {
        toast.show(e?.message || "Failed to revoke sessions", "error");
    } finally {
        revokingAll.value = false;
    }
}

defineExpose({ reload: load });
</script>

<template>
    <div class="space-y-4">
        <div v-if="loading" class="text-sm text-gray-500 py-4">
            Loading sessions…
        </div>
        <div v-else-if="sessions.length === 0" class="text-sm text-gray-500 py-4">
            No active sessions.
        </div>
        <div v-else class="divide-y divide-gray-100">
            <div
                v-for="session in sessions"
                :key="session.id"
                class="flex items-center justify-between py-3 gap-4"
            >
                <div class="min-w-0">
                    <div class="flex items-center gap-2">
                        <span class="text-sm font-medium truncate">{{
                            session.user_agent || "Unknown device"
                        }}</span>
                        <span
                            v-if="session.current"
                            class="text-xs font-medium text-green-600 bg-green-50 px-2 py-0.5 rounded-full"
                            >Current</span
                        >
                    </div>
                    <div class="text-xs text-gray-500 mt-0.5">
                        Started {{ formatDate(session.created_at) }} · Expires
                        {{ formatDate(session.expires_at) }}
                    </div>
                </div>
                <Button
                    v-if="!session.current"
                    label="Revoke"
                    icon="pi pi-sign-out"
                    severity="danger"
                    text
                    size="small"
                    :loading="revokingId === session.id"
                    @click="revoke(session)"
                />
            </div>
        </div>

        <div v-if="sessions.length > 0" class="pt-2 border-t border-gray-100">
            <Button
                label="Revoke all sessions"
                icon="pi pi-exclamation-triangle"
                severity="danger"
                outlined
                size="small"
                :loading="revokingAll"
                @click="revokeAll"
            />
        </div>
    </div>
</template>

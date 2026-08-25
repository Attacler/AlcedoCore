<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import Password from "primevue/password";

const authStore = useAuthStore(),
    router = useRouter(),
    route = useRoute();

const email = ref(""),
    password = ref("");

onMounted(async () => {
    await authStore.initialize();
    if (authStore.user) {
        const redirect = (route.query.redirect as string) || "/dashboard";
        router.replace(redirect);
    }
});

async function handleLogin() {
    const success = await authStore.login(email.value, password.value);
    if (success) {
        const redirect = (route.query.redirect as string) || "/dashboard";
        router.push(redirect);
    }
}
</script>

<template>
    <div class="min-h-screen flex items-center justify-center bg-gray-100">
        <div class="w-full max-w-md mx-4">
            <Card>
                <template #title>
                    <div class="text-center">
                        <div class="flex justify-center mb-4">
                            <span
                                class="material-symbols-outlined text-4xl text-blue-600"
                                >lock</span
                            >
                        </div>
                        <h2 class="text-xl font-semibold">Sign In</h2>
                        <p class="text-sm text-gray-500 mt-1">
                            Enter your credentials to access the admin panel
                        </p>
                    </div>
                </template>
                <template #content>
                    <form @submit.prevent="handleLogin" class="space-y-4">
                        <div class="flex flex-col gap-1">
                            <label
                                for="email"
                                class="text-sm font-medium text-gray-700"
                                >Email</label
                            >
                            <InputText
                                id="email"
                                v-model="email"
                                type="email"
                                placeholder="admin@example.com"
                                :disabled="authStore.loading"
                                :invalid="!!authStore.loginError"
                                fluid
                                @input="authStore.clearError()"
                            />
                        </div>
                        <div class="flex flex-col gap-1">
                            <label
                                for="password"
                                class="text-sm font-medium text-gray-700"
                                >Password</label
                            >
                            <Password
                                id="password"
                                v-model="password"
                                placeholder="Enter your password"
                                :feedback="false"
                                :disabled="authStore.loading"
                                :invalid="!!authStore.loginError"
                                toggleMask
                                fluid
                                @input="authStore.clearError()"
                            />
                        </div>
                        <Transition name="fade">
                            <div
                                v-if="authStore.loginError"
                                class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-3 text-sm flex items-center gap-2"
                            >
                                <span class="material-symbols-outlined text-lg"
                                    >error</span
                                >
                                <span>{{ authStore.loginError }}</span>
                            </div>
                        </Transition>
                        <Button
                            type="submit"
                            label="Sign In"
                            :loading="authStore.loading"
                            class="w-full"
                            size="large"
                        />
                    </form>
                </template>
            </Card>
            <p class="text-center text-xs text-gray-400 mt-6">
                Alcedo Admin Panel
            </p>
        </div>
    </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
    transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
    opacity: 0;
}
</style>

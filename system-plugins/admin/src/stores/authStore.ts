import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { User as UserInfo } from "@/types/user";
import { useMenuStore } from "./menuStore";

export const useAuthStore = defineStore("auth", () => {
    const user = ref<UserInfo | null>(null);
    const scopes = ref<string[]>([]);
    const isAdmin = ref(false);
    const loading = ref(false);
    const initialized = ref(false);
    const loginError = ref<string | null>(null);

    async function initialize() {
        loading.value = true;
        try {
            const res = await fetch("/api/auth/me", { credentials: "include" });
            if (res.ok) {
                const data = await res.json();
                user.value = data.user;
                scopes.value = data.scopes || [];
                isAdmin.value = data.is_admin === true;
                const menuStore = useMenuStore();
                menuStore.loadMyMenus();
            }
        } catch {
            user.value = null;
            scopes.value = [];
        } finally {
            loading.value = false;
            initialized.value = true;
        }
    }

    async function login(email: string, password: string): Promise<boolean> {
        loading.value = true;
        loginError.value = null;
        try {
            const res = await fetch("/api/auth/login", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                credentials: "include",
                body: JSON.stringify({ email, password }),
            });
            if (res.ok) {
                await initialize();
                return true;
            } else {
                const err = await res
                    .json()
                    .catch(() => ({ error: "Login failed" }));
                loginError.value = err.error || "Invalid email or password";
                return false;
            }
        } catch {
            loginError.value = "Network error — could not reach server";
            return false;
        } finally {
            loading.value = false;
        }
    }

    async function logout() {
        try {
            await fetch("/api/auth/logout", {
                method: "POST",
                credentials: "include",
            });
        } catch {
            // Proceed with local logout even if server call fails
        }
        user.value = null;
    }

    function hasScope(scope: string): boolean {
        return scopes.value.includes(scope);
    }

    function clearError() {
        loginError.value = null;
    }

    async function resolveLanding(): Promise<string> {
        if (isAdmin.value) {
            return "/apps";
        }
        // non-admin: fetch accessible apps
        try {
            const res = await fetch("/api/me/apps", { credentials: "include" });
            if (!res.ok) return "/apps";
            const data = await res.json();
            const apps = data?.data ?? data ?? [];
            if (apps.length === 1) {
                const a = apps[0];
                return `/app/${encodeURIComponent(a.api_name)}/${encodeURIComponent(a.version)}/dashboard`;
            }
            return "/apps";
        } catch {
            return "/apps";
        }
    }

    const displayName = computed(() => {
        return (
            user.value?.display_name ||
            user.value?.email?.split("@")[0] ||
            "User"
        );
    });

    const userInitial = computed(() => {
        return displayName.value.charAt(0).toUpperCase();
    });

    return {
        user,
        scopes,
        isAdmin,
        loading,
        initialized,
        loginError,
        initialize,
        login,
        logout,
        clearError,
        hasScope,
        resolveLanding,
        displayName,
        userInitial,
    };
});

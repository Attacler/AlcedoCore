import { defineStore } from "pinia";
import { ref } from "vue";
import { useAlcedoClient } from "../composables/useAlcedoClient";
import { withAsyncHandlingVoid } from "../utils/asyncUtils";

export interface ActivityLogEntry {
    id: string;
    action: string;
    description: string | null;
    target: string | null;
    item_id: string | null;
    metadata: Record<string, unknown> | null;
    diff: Record<string, unknown> | null;
    created_at: string;
}

export type ActivityTab = "items" | "system";

export interface ActivityFilters {
    dateRange: [Date | null, Date | null];
    actionType: string | null;
    itemId: string | null;
    collectionName: string | null;
}

export interface PaginationState {
    limit: number;
    offset: number;
    total: number;
}

export const useActivityLogStore = defineStore("activityLog", () => {
    const { client } = useAlcedoClient();

    const activeTab = ref<ActivityTab>("items");
    const filters = ref<ActivityFilters>({
        dateRange: [null, null],
        actionType: null,
        itemId: null,
        collectionName: null,
    });
    const logs = ref<ActivityLogEntry[]>([]);
    const pagination = ref<PaginationState>({ limit: 50, offset: 0, total: 0 });
    const loading = ref(false);
    const error = ref<string | null>(null);

    const TAG_DETAILS: {
        [key: string]: {
            serverity: string;
            dotColors: string;
            label: string;
            type: ActivityTab;
        };
    } = {
        item_created: {
            serverity: "success",
            dotColors: "bg-green-500 border-green-500",
            label: "Created",
            type: "items",
        },
        item_updated: {
            serverity: "info",
            dotColors: "bg-blue-500 border-blue-500",
            label: "Updated",
            type: "items",
        },
        item_deleted: {
            serverity: "danger",
            dotColors: "bg-red-500 border-red-500",
            label: "Deleted",
            type: "items",
        },

        setting_changed: {
            label: "Setting Changed",
            dotColors: "",
            serverity: "contrast",
            type: "system",
        },
        permission_created: {
            label: "Permission Created",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        permission_updated: {
            label: "Permission Updated",
            dotColors: "",
            serverity: "info",
            type: "system",
        },
        permission_deleted: {
            label: "Permission Deleted",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        permissions_deleted_collection: {
            label: "Removed permission from collection",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        policy_created: {
            label: "Policy Created",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        policy_updated: {
            label: "Policy Updated",
            dotColors: "",
            serverity: "info",
            type: "system",
        },
        policy_deleted: {
            label: "Policy Deleted",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        policy_assigned_to_plugin: {
            label: "Policy Assigned to plugin",
            dotColors: "",
            serverity: "primary",
            type: "system",
        },
        policy_unassigned_from_plugin: {
            label: "Policy Unassigned to plugin",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        collection_created: {
            label: "Collection Created",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        collection_updated: {
            label: "Collection Updated",
            dotColors: "",
            serverity: "info",
            type: "system",
        },
        collection_deleted: {
            label: "Collection Deleted",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        role_scopes_updated: {
            label: "Role scopes updated",
            dotColors: "",
            serverity: "primary",
            type: "system",
        },
        role_scope_removed: {
            label: "Role scopes removed",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        role_policy_assigned: {
            label: "Role policy assigned",
            dotColors: "",
            serverity: "primary",
            type: "system",
        },
        role_policy_removed: {
            label: "Role policy removed",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        role_created: {
            label: "Role Created",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        role_updated: {
            label: "Role Updated",
            dotColors: "",
            serverity: "info",
            type: "system",
        },
        role_deleted: {
            label: "Role Deleted",
            dotColors: "",
            serverity: "danger",
            type: "system",
        },
        developer_key_created: {
            label: "Developer key created",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        login_success: {
            label: "Login",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        logout: {
            label: "Logout",
            dotColors: "",
            serverity: "warn",
            type: "system",
        },
        user_role_assigned: {
            label: "Role assigned to user",
            dotColors: "",
            serverity: "success",
            type: "system",
        },
        user_role_removed: {
            label: "Role removed from user",
            dotColors: "",
            serverity: "warn",
            type: "system",
        },
    };

    function getSeverity(action: string): string {
        return TAG_DETAILS[action]?.serverity || "contrast";
    }

    function dotClass(action: string): string {
        return TAG_DETAILS[action]?.dotColors || "bg-gray-400 border-gray-400";
    }

    function formatActionLabel(action: string): string {
        return TAG_DETAILS[action]?.label || action;
    }

    function setTab(tab: ActivityTab) {
        activeTab.value = tab;
        pagination.value.offset = 0;
        logs.value = [];
        fetchLogs();
    }

    function setFilters(newFilters: Partial<ActivityFilters>) {
        Object.assign(filters.value, newFilters);
        pagination.value.offset = 0;
        fetchLogs();
    }

    function setPage(offset: number) {
        pagination.value.offset = offset;
        fetchLogs();
    }

    function resetFilters() {
        filters.value = {
            dateRange: [null, null],
            actionType: null,
            itemId: null,
            collectionName: null,
        };
        pagination.value.offset = 0;
        fetchLogs();
    }

    async function fetchLogs() {
        await withAsyncHandlingVoid(loading, error, async () => {
            const params = new URLSearchParams();
            params.set("limit", String(pagination.value.limit));
            params.set("offset", String(pagination.value.offset));
            if (filters.value.dateRange[0])
                params.set(
                    "start_date",
                    filters.value.dateRange[0].toISOString(),
                );
            if (filters.value.dateRange[1])
                params.set(
                    "end_date",
                    filters.value.dateRange[1].toISOString(),
                );
            if (filters.value.actionType)
                params.set("operation_type", filters.value.actionType);
            if (filters.value.itemId)
                params.set("item_id", filters.value.itemId);
            if (filters.value.collectionName)
                params.set("target", filters.value.collectionName);

            const fetcher =
                activeTab.value === "items"
                    ? client.activityLogs.listCollections
                    : client.activityLogs.listSystem;
            const response = (await fetcher(params)) as {
                data: ActivityLogEntry[];
                total: number;
                limit: number;
                offset: number;
            };
            logs.value = response.data;
            pagination.value = {
                limit: response.limit,
                offset: response.offset,
                total: response.total,
            };
        });
    }

    return {
        activeTab,
        filters,
        logs,
        pagination,
        loading,
        error,
        setTab,
        setFilters,
        setPage,
        resetFilters,
        fetchLogs,
        getSeverity,
        dotClass,
        formatActionLabel,
        TAG_DETAILS,
    };
});

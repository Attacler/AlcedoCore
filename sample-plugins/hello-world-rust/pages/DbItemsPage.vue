<script setup lang="ts">
import { ref, onMounted } from "vue";

const BASE = "/p/hello-world-rust/api";

interface Item {
    id: number;
    name: string;
    description: string | null;
    category: string;
    tags: string[] | null;
    created_at: string | null;
}

const items = ref<Item[]>([]);
const loading = ref(false);
const error = ref("");
const rawResponse = ref("");

async function fetchItems() {
    loading.value = true;
    error.value = "";
    rawResponse.value = "";
    try {
        const r = await fetch(`${BASE}/db/items`);
        const d = await r.json();
        rawResponse.value = JSON.stringify(d, null, 2);
        if (d.items && d.items.rows) {
            items.value = d.items.rows.map((row: any) => ({
                id: row.id,
                name: row.name,
                description: row.description || null,
                category: row.category || "general",
                tags: row.tags || null,
                created_at: row.created_at || null,
            }));
        }
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

async function runMigrations() {
    loading.value = true;
    error.value = "";
    rawResponse.value = "";
    try {
        const r = await fetch(`${BASE}/migrate`, { method: "POST" });
        const d = await r.json();
        rawResponse.value = JSON.stringify(d, null, 2);
        await fetchItems();
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

function fmtDate(d: string | null) {
    if (!d) return "—";
    return new Date(d).toLocaleDateString();
}

onMounted(fetchItems);
</script>

<template>
    <div class="db-items-page">
        <h1>DB Items</h1>
        <p class="subtitle">
            Database migrations and query proxy via <code>alcedo-sdk-rust</code>
        </p>

        <div v-if="error" class="error-msg">{{ error }}</div>

        <div class="toolbar">
            <button
                class="btn btn-primary"
                @click="fetchItems"
                :disabled="loading"
            >
                {{ loading ? "Loading..." : "Refresh Items" }}
            </button>
            <button
                class="btn btn-migrate"
                @click="runMigrations"
                :disabled="loading"
            >
                Run Migrations
            </button>
        </div>

        <div v-if="rawResponse" class="raw-response">
            <h3>API Response</h3>
            <pre>{{ rawResponse }}</pre>
        </div>

        <div v-if="!loading && items.length > 0" class="items-table-wrapper">
            <table class="items-table">
                <thead>
                    <tr>
                        <th>ID</th>
                        <th>Name</th>
                        <th>Description</th>
                        <th>Category</th>
                        <th>Tags</th>
                        <th>Created</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="item in items" :key="item.id">
                        <td class="cell-id">{{ item.id }}</td>
                        <td>{{ item.name }}</td>
                        <td>{{ item.description || "—" }}</td>
                        <td>
                            <span class="badge">{{ item.category }}</span>
                        </td>
                        <td>
                            <span v-if="item.tags && item.tags.length">
                                <span
                                    v-for="tag in item.tags"
                                    :key="tag"
                                    class="tag"
                                    >{{ tag }}</span
                                >
                            </span>
                            <span v-else class="cell-muted">—</span>
                        </td>
                        <td class="cell-muted">
                            {{ fmtDate(item.created_at) }}
                        </td>
                    </tr>
                    <tr v-if="items.length === 0 && !loading">
                        <td colspan="6" class="empty-row">
                            No items found. Run migrations to create the table.
                        </td>
                    </tr>
                </tbody>
            </table>
        </div>

        <div v-if="loading && items.length === 0" class="loading-bar">
            Loading...
        </div>
    </div>
</template>

<style>
.db-items-page {
    padding: 1.5rem;
    max-width: 960px;
    margin: 0 auto;
}
.db-items-page h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: #1f2937;
    margin: 0;
}
.subtitle {
    color: #6b7280;
    font-size: 0.875rem;
    margin-top: 0.25rem;
    margin-bottom: 1rem;
}
.subtitle code {
    background: #f3f4f6;
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
}

.toolbar {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
}
.btn {
    padding: 0.5rem 1rem;
    border-radius: 0.375rem;
    border: none;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
}
.btn:active {
    transform: scale(0.97);
}
.btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
}
.btn-primary {
    background: #3b82f6;
    color: white;
}
.btn-primary:hover:not(:disabled) {
    background: #2563eb;
}
.btn-migrate {
    background: #fef3c7;
    color: #d97706;
}
.btn-migrate:hover:not(:disabled) {
    background: #fde68a;
}

.raw-response {
    background: #1f2937;
    color: #e5e7eb;
    padding: 0.75rem;
    border-radius: 0.375rem;
    margin-bottom: 1rem;
}
.raw-response h3 {
    font-size: 0.75rem;
    font-weight: 600;
    color: #9ca3af;
    margin: 0 0 0.5rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
}
.raw-response pre {
    font-size: 0.75rem;
    font-family: monospace;
    white-space: pre-wrap;
    margin: 0;
}

.items-table-wrapper {
    overflow-x: auto;
}
.items-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.875rem;
    min-width: 600px;
}
.items-table th {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 2px solid #e5e7eb;
    color: #6b7280;
    font-weight: 600;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    white-space: nowrap;
}
.items-table td {
    padding: 0.5rem;
    border-bottom: 1px solid #f3f4f6;
    color: #374151;
}
.items-table tr:hover td {
    background: #f9fafb;
}

.cell-id {
    font-family: monospace;
    color: #9ca3af;
    width: 3rem;
}
.cell-muted {
    color: #9ca3af;
    font-size: 0.8rem;
}
.badge {
    display: inline-block;
    padding: 0.125rem 0.5rem;
    border-radius: 9999px;
    font-size: 0.75rem;
    font-weight: 500;
    background: #f3f4f6;
    color: #6b7280;
    white-space: nowrap;
}
.tag {
    display: inline-block;
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
    font-size: 0.7rem;
    font-weight: 500;
    background: #dbeafe;
    color: #2563eb;
    margin-right: 0.25rem;
}

.empty-row {
    text-align: center;
    padding: 2rem !important;
    color: #9ca3af;
}
.error-msg {
    color: #dc2626;
    font-size: 0.875rem;
    padding: 0.5rem;
    background: #fef2f2;
    border-radius: 0.25rem;
    margin-bottom: 0.75rem;
}
.loading-bar {
    text-align: center;
    padding: 2rem;
    color: #6b7280;
    font-size: 0.875rem;
}
</style>

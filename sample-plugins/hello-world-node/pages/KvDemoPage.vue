<script setup lang="ts">
import { ref } from "vue";

const BASE = "/p/hello-world-node/api/kv";

// Single key operations
const key = ref("demo-key");
const value = ref("Hello KV Store!");
const ttl = ref<number | null>(null);
const result = ref("");
const error = ref("");
const loading = ref(false);

// Batch operations
const batchKeys = ref("key1, key2, key3");
const batchResult = ref("");
const listPrefix = ref("");
const listResult = ref("");

async function getKey() {
    loading.value = true;
    result.value = "";
    error.value = "";
    try {
        const r = await fetch(`${BASE}/${key.value}`);
        const d = await r.json();
        result.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

async function setKey() {
    loading.value = true;
    result.value = "";
    error.value = "";
    try {
        const body: any = { value: value.value };
        if (ttl.value !== null && ttl.value > 0) body.ttl = ttl.value;
        const r = await fetch(`${BASE}/${key.value}`, {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
        });
        const d = await r.json();
        result.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

async function deleteKey() {
    loading.value = true;
    result.value = "";
    error.value = "";
    try {
        const r = await fetch(`${BASE}/${key.value}`, { method: "DELETE" });
        const d = await r.json();
        result.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

async function getTtl() {
    loading.value = true;
    result.value = "";
    error.value = "";
    try {
        const r = await fetch(`${BASE}/${key.value}/ttl`);
        const d = await r.json();
        result.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

// ─── Batch operations ──────────────────────────────────────────────────

async function batchGet() {
    batchResult.value = "";
    error.value = "";
    try {
        const keys = batchKeys.value
            .split(",")
            .map((k) => k.trim())
            .filter(Boolean);
        const r = await fetch(`${BASE}/batch-get`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ keys }),
        });
        const d = await r.json();
        batchResult.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    }
}

async function batchSet() {
    batchResult.value = "";
    error.value = "";
    try {
        const keys = batchKeys.value
            .split(",")
            .map((k) => k.trim())
            .filter(Boolean);
        const pairs = keys.map((k, i) => ({
            key: k,
            value: `batch-value-${i + 1}`,
        }));
        const r = await fetch(`${BASE}/batch-set`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ pairs }),
        });
        const d = await r.json();
        batchResult.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    }
}

async function batchDelete() {
    batchResult.value = "";
    error.value = "";
    try {
        const keys = batchKeys.value
            .split(",")
            .map((k) => k.trim())
            .filter(Boolean);
        const r = await fetch(`${BASE}/batch-delete`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ keys }),
        });
        const d = await r.json();
        batchResult.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    }
}

// ─── List ──────────────────────────────────────────────────────────────

async function listKeys() {
    listResult.value = "";
    error.value = "";
    try {
        const prefix = listPrefix.value
            ? `?prefix=${encodeURIComponent(listPrefix.value)}`
            : "";
        const r = await fetch(`${BASE}/list${prefix}`);
        const d = await r.json();
        listResult.value = JSON.stringify(d, null, 2);
    } catch (e: any) {
        error.value = e.message;
    }
}
</script>

<template>
    <div class="kv-demo-page">
        <h1>KV Store Demo</h1>
        <p class="subtitle">
            Demonstrates CRUD, TTL, batch, and list operations via
            <code>alcedocore-sdk-node</code>
        </p>

        <div v-if="error" class="error-msg">{{ error }}</div>

        <!-- Single Key Operations -->
        <section class="section">
            <h2>Single Key Operations</h2>
            <div class="form-row">
                <label class="form-label">Key:</label>
                <input v-model="key" class="input" placeholder="key name" />
            </div>
            <div class="form-row">
                <label class="form-label">Value:</label>
                <input
                    v-model="value"
                    class="input flex-1"
                    placeholder="value"
                />
            </div>
            <div class="form-row">
                <label class="form-label">TTL (sec):</label>
                <input
                    v-model.number="ttl"
                    type="number"
                    class="input w-24"
                    placeholder="optional"
                />
            </div>
            <div class="btn-row">
                <button class="btn btn-get" :disabled="loading" @click="getKey">
                    GET
                </button>
                <button class="btn btn-set" :disabled="loading" @click="setKey">
                    PUT (set)
                </button>
                <button
                    class="btn btn-del"
                    :disabled="loading"
                    @click="deleteKey"
                >
                    DELETE
                </button>
                <button class="btn btn-ttl" :disabled="loading" @click="getTtl">
                    TTL
                </button>
            </div>
            <pre v-if="result" class="result-block">{{ result }}</pre>
        </section>

        <!-- Batch Operations -->
        <section class="section">
            <h2>Batch Operations</h2>
            <div class="form-row">
                <label class="form-label">Keys (comma-separated):</label>
                <input
                    v-model="batchKeys"
                    class="input flex-1"
                    placeholder="key1, key2, key3"
                />
            </div>
            <div class="btn-row">
                <button class="btn btn-get" @click="batchGet">Batch GET</button>
                <button class="btn btn-set" @click="batchSet">Batch SET</button>
                <button class="btn btn-del" @click="batchDelete">
                    Batch DELETE
                </button>
            </div>
            <pre v-if="batchResult" class="result-block">{{ batchResult }}</pre>
        </section>

        <!-- List -->
        <section class="section">
            <h2>List Keys</h2>
            <div class="form-row">
                <label class="form-label">Prefix:</label>
                <input
                    v-model="listPrefix"
                    class="input flex-1"
                    placeholder="optional prefix filter"
                />
            </div>
            <div class="btn-row">
                <button class="btn btn-ttl" @click="listKeys">List</button>
            </div>
            <pre v-if="listResult" class="result-block">{{ listResult }}</pre>
        </section>
    </div>
</template>

<style>
.kv-demo-page {
    padding: 1.5rem;
    max-width: 800px;
    margin: 0 auto;
}
.kv-demo-page h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: #1f2937;
    margin: 0;
}
.subtitle {
    color: #6b7280;
    font-size: 0.875rem;
    margin-top: 0.25rem;
    margin-bottom: 1.5rem;
}
.subtitle code {
    background: #f3f4f6;
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
}

.section {
    background: #f9fafb;
    border: 1px solid #e5e7eb;
    border-radius: 0.5rem;
    padding: 1rem;
    margin-bottom: 1rem;
}
.section h2 {
    font-size: 1.125rem;
    font-weight: 600;
    color: #374151;
    margin: 0 0 0.75rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #e5e7eb;
}

.form-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
}
.form-label {
    font-size: 0.875rem;
    font-weight: 500;
    color: #4b5563;
    min-width: 120px;
}
.input {
    padding: 0.375rem 0.5rem;
    border: 1px solid #d1d5db;
    border-radius: 0.25rem;
    font-size: 0.875rem;
}
.input:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.15);
}
.flex-1 {
    flex: 1;
}
.w-24 {
    width: 6rem;
}

.btn-row {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.75rem;
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
.btn-get {
    background: #dbeafe;
    color: #2563eb;
}
.btn-get:hover:not(:disabled) {
    background: #bfdbfe;
}
.btn-set {
    background: #d1fae5;
    color: #059669;
}
.btn-set:hover:not(:disabled) {
    background: #a7f3d0;
}
.btn-del {
    background: #fef2f2;
    color: #dc2626;
}
.btn-del:hover:not(:disabled) {
    background: #fecaca;
}
.btn-ttl {
    background: #fef3c7;
    color: #d97706;
}
.btn-ttl:hover:not(:disabled) {
    background: #fde68a;
}

.result-block {
    background: #1f2937;
    color: #e5e7eb;
    padding: 0.75rem;
    border-radius: 0.375rem;
    font-size: 0.75rem;
    font-family: monospace;
    overflow-x: auto;
    margin-top: 0.75rem;
    white-space: pre-wrap;
}

.error-msg {
    color: #dc2626;
    font-size: 0.875rem;
    margin-bottom: 0.75rem;
    padding: 0.5rem;
    background: #fef2f2;
    border-radius: 0.25rem;
}
</style>

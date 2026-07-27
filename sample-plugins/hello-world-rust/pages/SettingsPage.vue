<script setup lang="ts">
import { ref, onMounted } from "vue";

const BASE = "/p/hello-world-rust/api";

interface PluginSettings {
    greeting?: string;
    refresh_interval?: number;
    theme_color?: string;
}

const settings = ref<PluginSettings | null>(null);
const loading = ref(false);
const error = ref("");

async function fetchSettings() {
    loading.value = true;
    error.value = "";
    try {
        const r = await fetch(`${BASE}/settings`);
        const d = await r.json();
        settings.value = d.settings || d;
    } catch (e: any) {
        error.value = e.message;
    } finally {
        loading.value = false;
    }
}

onMounted(fetchSettings);
</script>

<template>
    <div class="settings-page">
        <h1>Plugin Settings</h1>
        <p class="subtitle">
            Demonstrates settings access via
            <code>alcedo-sdk-rust</code> settings resource
        </p>

        <div v-if="error" class="error-msg">{{ error }}</div>
        <div v-if="loading" class="loading-bar">Loading settings...</div>

        <div v-if="settings" class="settings-card">
            <h2>Current Configuration</h2>
            <table class="settings-table">
                <tbody>
                    <tr>
                        <td class="setting-name">Greeting Message</td>
                        <td class="setting-value">
                            {{ settings.greeting || "(not set)" }}
                        </td>
                    </tr>
                    <tr>
                        <td class="setting-name">Refresh Interval</td>
                        <td class="setting-value">
                            {{
                                settings.refresh_interval ?? "(not set)"
                            }}
                            seconds
                        </td>
                    </tr>
                    <tr>
                        <td class="setting-name">Theme Color</td>
                        <td class="setting-value">
                            <span
                                class="theme-badge"
                                :style="{
                                    background:
                                        settings.theme_color || '#e5e7eb',
                                }"
                            >
                                {{ settings.theme_color || "(not set)" }}
                            </span>
                        </td>
                    </tr>
                </tbody>
            </table>
            <button class="btn btn-refresh" @click="fetchSettings">
                Refresh
            </button>
        </div>

        <div v-if="!settings && !loading && !error" class="empty-state">
            No settings loaded.
            <button class="btn btn-refresh" @click="fetchSettings">
                Load settings
            </button>
        </div>
    </div>
</template>

<style>
.settings-page {
    padding: 1.5rem;
    max-width: 640px;
    margin: 0 auto;
}
.settings-page h1 {
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

.settings-card {
    background: #f9fafb;
    border: 1px solid #e5e7eb;
    border-radius: 0.5rem;
    padding: 1.5rem;
}
.settings-card h2 {
    font-size: 1.125rem;
    font-weight: 600;
    color: #374151;
    margin: 0 0 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #e5e7eb;
}

.settings-table {
    width: 100%;
    border-collapse: collapse;
}
.settings-table tr {
    border-bottom: 1px solid #f3f4f6;
}
.settings-table td {
    padding: 0.75rem 0.5rem;
    font-size: 0.875rem;
}
.setting-name {
    font-weight: 500;
    color: #4b5563;
    width: 40%;
}
.setting-value {
    color: #374151;
    font-family: monospace;
}

.theme-badge {
    display: inline-block;
    padding: 0.125rem 0.5rem;
    border-radius: 0.25rem;
    font-size: 0.75rem;
    font-weight: 500;
    color: white;
    text-transform: capitalize;
}

.btn {
    padding: 0.5rem 1rem;
    border-radius: 0.375rem;
    border: none;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
    margin-top: 1rem;
}
.btn:active {
    transform: scale(0.97);
}
.btn-refresh {
    background: #3b82f6;
    color: white;
}
.btn-refresh:hover {
    background: #2563eb;
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
.empty-state {
    text-align: center;
    padding: 2rem;
    color: #9ca3af;
    font-size: 0.875rem;
}
.empty-state .btn {
    margin-top: 0.5rem;
}
</style>

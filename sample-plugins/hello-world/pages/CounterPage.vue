<script setup lang="ts">
import { ref, onMounted } from "vue";

const count = ref(0);
const loading = ref(false);
const step = ref(1);
const error = ref("");

const BASE = "/p/hello-world/api/counter";

async function fetchCount() {
    try {
        const r = await fetch(`${BASE}/value`);
        const d = await r.json();
        count.value = d.count;
    } catch {
        error.value = "Failed to load count";
    }
}

async function increment() {
    loading.value = true;
    error.value = "";
    try {
        const r = await fetch(`${BASE}/increment`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ step: step.value }),
        });
        const d = await r.json();
        if (d.error) {
            error.value = d.error;
            return;
        }
        count.value = d.count;
    } catch {
        error.value = "Failed to increment";
    } finally {
        loading.value = false;
    }
}

async function decrement() {
    loading.value = true;
    error.value = "";
    try {
        const r = await fetch(`${BASE}/decrement`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ step: step.value }),
        });
        const d = await r.json();
        if (d.error) {
            error.value = d.error;
            return;
        }
        count.value = d.count;
    } catch {
        error.value = "Failed to decrement";
    } finally {
        loading.value = false;
    }
}

async function reset() {
    // Reset to 0 by setting the counter via the KV store
    loading.value = true;
    error.value = "";
    try {
        const r = await fetch(`${BASE}/value`);
        const d = await r.json();
        const current = d.count;
        const ops = current > 0 ? "decrement" : "increment";
        for (let i = 0; i < Math.abs(current); i++) {
            await fetch(`${BASE}/${ops}`, { method: "POST" });
        }
        count.value = 0;
    } catch {
        error.value = "Failed to reset";
    } finally {
        loading.value = false;
    }
}

function setStep(s: number) {
    step.value = s;
}

onMounted(fetchCount);
</script>

<template>
    <div class="counter-page">
        <h1>Counter</h1>
        <p class="subtitle">Persistent KV counter, click to change</p>

        <div class="count-display">{{ count }}</div>

        <div class="button-row">
            <button class="btn btn-dec" :disabled="loading" @click="decrement">
                -
            </button>
            <button class="btn btn-reset" :disabled="loading" @click="reset">
                Reset
            </button>
            <button class="btn btn-inc" :disabled="loading" @click="increment">
                +
            </button>
        </div>

        <div class="step-row">
            <span class="step-label">Step:</span>
            <div class="step-buttons">
                <button
                    class="step-btn"
                    :class="step === 1 ? 'step-active' : 'step-inactive'"
                    @click="setStep(1)"
                >
                    1
                </button>
                <button
                    class="step-btn"
                    :class="step === 5 ? 'step-active' : 'step-inactive'"
                    @click="setStep(5)"
                >
                    5
                </button>
                <button
                    class="step-btn"
                    :class="step === 10 ? 'step-active' : 'step-inactive'"
                    @click="setStep(10)"
                >
                    10
                </button>
            </div>
        </div>

        <p v-if="error" class="error">{{ error }}</p>

        <p class="footer-link">
            Link to:
            <a href="/admin#/p/hello-world/index" class="link">Hello</a>
        </p>
    </div>
</template>

<style>
.counter-page {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 60vh;
    padding: 1.5rem;
}

.counter-page h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: #1f2937;
    margin-bottom: 0.5rem;
}

.subtitle {
    color: #6b7280;
    margin-bottom: 2rem;
}

.count-display {
    font-size: 5rem;
    font-weight: 700;
    color: #1f2937;
    margin-bottom: 2rem;
    user-select: none;
}

.button-row {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1.5rem;
}

.btn {
    transition: all 0.15s;
    cursor: pointer;
}

.btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
}

.btn:active:not(:disabled) {
    transform: scale(0.95);
}

.btn-dec {
    width: 4rem;
    height: 4rem;
    border-radius: 9999px;
    background: #fef2f2;
    color: #dc2626;
    font-size: 1.875rem;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
}

.btn-dec:hover:not(:disabled) {
    background: #fecaca;
}

.btn-reset {
    padding: 0.75rem 1.5rem;
    border-radius: 0.5rem;
    background: #f3f4f6;
    color: #4b5563;
    font-size: 0.875rem;
    font-weight: 500;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.btn-reset:hover:not(:disabled) {
    background: #e5e7eb;
}

.btn-inc {
    width: 4rem;
    height: 4rem;
    border-radius: 9999px;
    background: #f0fdf4;
    color: #16a34a;
    font-size: 1.875rem;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
}

.btn-inc:hover:not(:disabled) {
    background: #bbf7d0;
}

.step-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
}

.step-label {
    font-size: 0.875rem;
    color: #6b7280;
}

.step-buttons {
    display: flex;
    gap: 0.5rem;
}

.step-btn {
    padding: 0.5rem 1rem;
    border-radius: 0.5rem;
    font-size: 0.875rem;
    font-weight: 500;
    transition: all 0.15s;
    cursor: pointer;
}

.step-active {
    background: #3b82f6;
    color: white;
    box-shadow: 0 4px 6px -1px rgba(59, 130, 246, 0.3);
}

.step-inactive {
    background: #f3f4f6;
    color: #4b5563;
}

.step-inactive:hover {
    background: #e5e7eb;
}

.error {
    margin-top: 1rem;
    color: #dc2626;
    font-size: 0.875rem;
}

.footer-link {
    margin-top: 3rem;
    font-size: 0.875rem;
    color: #9ca3af;
}

.link {
    color: #3b82f6;
    text-decoration: underline;
}

.link:hover {
    color: #1d4ed8;
}
</style>

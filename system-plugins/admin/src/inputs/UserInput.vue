<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import Select from "primevue/select";

defineProps<{
    field?: any;
    modelValue?: any;
    invalid?: boolean | string;
    readonly?: boolean;
}>();

const emit = defineEmits<{ "update:modelValue": [value: any] }>();

const { client } = useAlcedoClient();

const options = ref<{ label: string; value: string }[]>([]),
    loading = ref(false);

function label(user: any): string {
    return user.display_name || user.email || user.id;
}

async function loadOptions() {
    loading.value = true;
    try {
        const { data } = await client.lookup.users();
        options.value = (data ?? []).map((u) => ({
            label: label(u),
            value: u.id,
        }));
    } catch {
        options.value = [];
    } finally {
        loading.value = false;
    }
}

onMounted(loadOptions);
</script>

<template>
    <Select
        :model-value="modelValue"
        :options="options"
        option-label="label"
        option-value="value"
        :loading="loading"
        :disabled="readonly"
        :invalid="!!invalid"
        filter
        show-clear
        placeholder="Select user"
        class="w-full"
        @update:model-value="emit('update:modelValue', $event)"
    />
</template>

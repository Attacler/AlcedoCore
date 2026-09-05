<script setup lang="ts">
const props = defineProps<{
    field?: any;
    invalid?: boolean | string;
    readonly?: boolean;
}>();
const value = defineModel<string>();

const { options } = props.field;
</script>

<template>
    <Textarea
        v-model="value"
        :placeholder="field?.default_value || 'Multi-Line'"
        :invalid="!!invalid"
        :readonly="readonly"
        :rows="options?.rows || undefined"
        :maxlength="options?.max_length || undefined"
        :autoResize="!options.rows"
        fluid
        class="text-sm"
    />
</template>

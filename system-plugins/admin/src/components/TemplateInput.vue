<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";

const props = defineProps<{
    modelValue: string;
    fields: FieldDefinition[];
    placeholder?: string;
}>();

const emit = defineEmits<{
    "update:modelValue": [value: string];
}>();

const editorRef = ref<HTMLDivElement | null>(null),
    popoverRef = ref<HTMLDivElement | null>(null),
    tagPopupRef = ref<HTMLDivElement | null>(null);

const showFieldPopover = ref(false),
    showTagPopup = ref(false),
    isFocused = ref(false),
    activeFieldTag = ref<HTMLElement | null>(null),
    lastEmittedValue = ref(""),
    pendingFilter = ref(""),
    popoverStyle = ref<Record<string, string>>({}),
    tagPopupStyle = ref<Record<string, string>>({});

const filteredFields = computed(() => {
    const q = pendingFilter.value.trim().toLowerCase();
    if (!q) return props.fields;
    return props.fields.filter(
        (f) =>
            (f.display_name || f.name).toLowerCase().includes(q) ||
            f.name.toLowerCase().includes(q),
    );
});

function parseTemplate(
    value: string,
): Array<{ type: "text" | "field"; value: string }> {
    const segments: Array<{ type: "text" | "field"; value: string }> = [];
    let lastIndex = 0;
    const regex = /\{\{\s*([^}]+)\s*\}\}/g;
    let match: RegExpExecArray | null;
    while ((match = regex.exec(value)) !== null) {
        if (match.index > lastIndex) {
            segments.push({
                type: "text",
                value: value.slice(lastIndex, match.index),
            });
        }
        segments.push({ type: "field", value: match[1].trim() });
        lastIndex = match.index + match[0].length;
    }
    if (lastIndex < value.length) {
        segments.push({ type: "text", value: value.slice(lastIndex) });
    }
    return segments;
}

function escapeHtml(str: string): string {
    return str
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#039;");
}

function createFieldChip(name: string): HTMLElement {
    const chip = document.createElement("span");
    chip.className =
        "inline-flex items-center gap-1 bg-blue-100 text-blue-700 rounded px-1.5 py-0.5 text-xs font-medium leading-none";
    chip.dataset.field = name;
    chip.contentEditable = "false";
    chip.innerHTML = `${escapeHtml(name)}<span class="cursor-pointer hover:text-blue-900 ml-0.5 leading-none" data-action="remove">&times;</span>`;
    return chip;
}

function renderTemplate(value: string) {
    if (!editorRef.value) return;
    const segments = parseTemplate(value);
    editorRef.value.innerHTML = segments
        .map((seg) => {
            if (seg.type === "field") {
                return createFieldChip(seg.value).outerHTML;
            }
            return escapeHtml(seg.value);
        })
        .join("");
}

function reconstructValue(): string {
    if (!editorRef.value) return "";
    let result = "";
    function walkNodes(nodes: NodeListOf<ChildNode>) {
        for (const node of nodes) {
            if (node.nodeType === Node.TEXT_NODE) {
                result += node.textContent || "";
            } else if (node instanceof HTMLElement) {
                if (node.dataset.field) {
                    result += `{{${node.dataset.field}}}`;
                } else if (node instanceof HTMLBRElement) {
                    result += "\n";
                } else {
                    walkNodes(node.childNodes);
                }
            }
        }
    }
    walkNodes(editorRef.value.childNodes);
    return result;
}

/** Partial field name being typed after an unclosed "{{", or null if none. */
function getPendingToken(): string | null {
    const sel = window.getSelection();
    if (!sel || !sel.rangeCount) return null;
    const node = sel.anchorNode;
    if (!node || node.nodeType !== Node.TEXT_NODE) return null;
    const text = node.textContent || "";
    const before = text.slice(0, sel.anchorOffset);
    const lastOpen = before.lastIndexOf("{{");
    if (lastOpen === -1) return null;
    if (before.lastIndexOf("}}") > lastOpen) return null;
    return before.slice(lastOpen + 2);
}

function checkPendingBraces() {
    if (!editorRef.value) return;
    const token = getPendingToken();
    if (token !== null) {
        if (!showFieldPopover.value) {
            showFieldPopover.value = true;
        }
        pendingFilter.value = token;
        positionPopover();
    } else {
        showFieldPopover.value = false;
        pendingFilter.value = "";
    }
}

function positionPopover() {
    const sel = window.getSelection();
    if (!sel || !sel.rangeCount || !editorRef.value) return;
    try {
        const range = sel.getRangeAt(0);
        const rect = range.getBoundingClientRect();
        const editorRect = editorRef.value.getBoundingClientRect();
        popoverStyle.value = {
            top: `${rect.bottom - editorRect.top + 4}px`,
            left: `${Math.max(0, rect.left - editorRect.left)}px`,
        };
    } catch {
        const editorRect = editorRef.value.getBoundingClientRect();
        popoverStyle.value = {
            top: `${editorRect.height + 4}px`,
            left: "0px",
        };
    }
}

function positionTagPopup(chip: HTMLElement) {
    if (!editorRef.value) return;
    const chipRect = chip.getBoundingClientRect();
    const editorRect = editorRef.value.getBoundingClientRect();
    tagPopupStyle.value = {
        top: `${chipRect.bottom - editorRect.top + 2}px`,
        left: `${chipRect.left - editorRect.left}px`,
    };
}

function insertField(name: string) {
    if (!editorRef.value) return;

    if (activeFieldTag.value && showTagPopup.value) {
        const chip = activeFieldTag.value;
        const newChip = createFieldChip(name);
        chip.replaceWith(newChip);
        showFieldPopover.value = false;
        showTagPopup.value = false;
        activeFieldTag.value = null;
        pendingFilter.value = "";
        const value = reconstructValue();
        lastEmittedValue.value = value;
        emit("update:modelValue", value);
        return;
    }

    const sel = window.getSelection();
    if (!sel || !sel.rangeCount || !editorRef.value.contains(sel.anchorNode)) {
        const chip = createFieldChip(name);
        editorRef.value.appendChild(chip);
        const range = document.createRange();
        const lastChild = editorRef.value.lastChild;
        if (lastChild) {
            range.setStartAfter(lastChild);
            range.collapse(true);
        } else {
            range.setStartAfter(chip);
            range.collapse(true);
        }
        sel?.removeAllRanges();
        sel?.addRange(range);
        showFieldPopover.value = false;
        pendingFilter.value = "";
        const value = reconstructValue();
        lastEmittedValue.value = value;
        emit("update:modelValue", value);
        return;
    }

    const range = sel.getRangeAt(0);
    const startContainer = range.startContainer;
    if (startContainer.nodeType === Node.TEXT_NODE) {
        const text = startContainer.textContent || "";
        const offset = range.startOffset;
        const beforeCursor = text.slice(0, offset);
        const match = beforeCursor.match(/\{\{[^}]*$/);
        if (match) {
            startContainer.textContent =
                text.slice(0, match.index) + text.slice(offset);
            range.setStart(startContainer, match.index);
            range.collapse(true);
        }
    }

    range.deleteContents();
    const chip = createFieldChip(name);
    range.insertNode(chip);
    range.setStartAfter(chip);
    range.setEndAfter(chip);
    sel.removeAllRanges();
    sel.addRange(range);
    showFieldPopover.value = false;
    pendingFilter.value = "";
    const value = reconstructValue();
    lastEmittedValue.value = value;
    emit("update:modelValue", value);
}

function removeFieldTag() {
    if (!activeFieldTag.value) return;
    activeFieldTag.value.remove();
    showTagPopup.value = false;
    activeFieldTag.value = null;
    editorRef.value?.focus();
    const value = reconstructValue();
    lastEmittedValue.value = value;
    emit("update:modelValue", value);
}

function changeFieldTag() {
    if (!activeFieldTag.value) return;
    showTagPopup.value = false;
    positionPopoverForChange(activeFieldTag.value);
    showFieldPopover.value = true;
}

function positionPopoverForChange(chip: HTMLElement) {
    if (!editorRef.value) return;
    const chipRect = chip.getBoundingClientRect();
    const editorRect = editorRef.value.getBoundingClientRect();
    popoverStyle.value = {
        top: `${chipRect.bottom - editorRect.top + 4}px`,
        left: `${chipRect.left - editorRect.left}px`,
    };
}

function onInput() {
    const value = reconstructValue();
    lastEmittedValue.value = value;
    emit("update:modelValue", value);
    checkPendingBraces();
}

function onEditorClick(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (target.dataset.action === "remove") {
        event.preventDefault();
        event.stopPropagation();
        const chip = target.closest("[data-field]") as HTMLElement;
        if (chip) {
            chip.remove();
            editorRef.value?.focus();
            const value = reconstructValue();
            lastEmittedValue.value = value;
            emit("update:modelValue", value);
        }
        return;
    }
    const chip = target.closest("[data-field]") as HTMLElement;
    if (chip) {
        event.preventDefault();
        activeFieldTag.value = chip;
        showTagPopup.value = true;
        positionTagPopup(chip);
        return;
    }
    showTagPopup.value = false;
    activeFieldTag.value = null;
}

function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
        if (showTagPopup.value) {
            showTagPopup.value = false;
            activeFieldTag.value = null;
            event.preventDefault();
        } else if (showFieldPopover.value) {
            showFieldPopover.value = false;
            event.preventDefault();
        }
    }
}

function onBlur() {
    isFocused.value = false;
}

function onPaste(event: ClipboardEvent) {
    event.preventDefault();
    const text = event.clipboardData?.getData("text/plain");
    if (text) {
        document.execCommand("insertText", false, text);
    }
}

function onDocumentMousedown(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (showFieldPopover.value && popoverRef.value) {
        if (
            !popoverRef.value.contains(target) &&
            !editorRef.value?.contains(target)
        ) {
            showFieldPopover.value = false;
        }
    }
    if (showTagPopup.value && tagPopupRef.value) {
        if (
            !tagPopupRef.value.contains(target) &&
            !target.closest("[data-field]")
        ) {
            showTagPopup.value = false;
            activeFieldTag.value = null;
        }
    }
}

watch(
    () => props.modelValue,
    (newVal) => {
        if (newVal === lastEmittedValue.value) return;
        renderTemplate(newVal || "");
    },
);

onMounted(() => {
    renderTemplate(props.modelValue || "");
    document.addEventListener("mousedown", onDocumentMousedown);
});

onUnmounted(() => {
    document.removeEventListener("mousedown", onDocumentMousedown);
});
</script>

<template>
    <div class="relative">
        <div
            ref="editorRef"
            contenteditable="true"
            class="border border-gray-300 rounded-md px-3 py-2 text-sm bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500 min-h-[2.25rem] cursor-text whitespace-pre-wrap break-words"
            :data-placeholder="placeholder"
            @input="onInput"
            @click="onEditorClick"
            @keydown="onKeydown"
            @focus="isFocused = true"
            @blur="onBlur"
            @paste="onPaste"
        ></div>
        <span
            v-if="!modelValue && !isFocused"
            class="absolute left-3 top-2 text-sm text-gray-400 pointer-events-none select-none truncate max-w-[calc(100%-1.5rem)]"
            >{{ placeholder }}</span
        >
        <div
            v-if="showFieldPopover"
            ref="popoverRef"
            class="absolute z-50 bg-white border border-gray-200 rounded-lg shadow-lg max-h-48 overflow-y-auto min-w-[180px]"
            :style="popoverStyle"
        >
            <template v-if="filteredFields.length > 0">
                <div
                    v-for="field in filteredFields"
                    :key="field.name"
                    class="px-3 py-1.5 text-sm text-gray-700 hover:bg-blue-50 hover:text-blue-700 cursor-pointer flex items-center gap-2"
                    @mousedown.prevent="insertField(field.name)"
                >
                    <span><FieldNameLabel :field="field" /></span>
                    <span class="text-xs text-gray-400 ml-auto">{{
                        field.type
                    }}</span>
                </div>
            </template>
            <div v-else class="px-3 py-2 text-sm text-gray-400">
                {{
                    fields.length > 0
                        ? `No fields match "${pendingFilter}"`
                        : "No fields available"
                }}
            </div>
        </div>
        <div
            v-if="showTagPopup"
            ref="tagPopupRef"
            class="absolute z-50 bg-white border border-gray-200 rounded-lg shadow-lg min-w-[140px]"
            :style="tagPopupStyle"
        >
            <div
                class="px-3 py-1.5 text-sm text-gray-700 hover:bg-blue-50 cursor-pointer"
                @mousedown.prevent="removeFieldTag"
            >
                Remove
            </div>
            <div
                class="px-3 py-1.5 text-sm text-gray-700 hover:bg-blue-50 cursor-pointer"
                @mousedown.prevent="changeFieldTag"
            >
                Change to...
            </div>
        </div>
    </div>
</template>

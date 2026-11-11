import { describe, it, expect, vi } from "vitest";
import { mount } from "@vue/test-utils";
import FilterBuilder from "@/components/FilterBuilder.vue";
import type { FilterCondition } from "@/types/filters";

vi.mock("@/stores/collections", () => ({
    useCollectionsStore: () => ({
        getCollection: vi.fn().mockResolvedValue({
            fields: [
                { name: "id", type: "int" },
                { name: "email", type: "string" },
                { name: "score", type: "float" },
            ],
        }),
    }),
}));

vi.mock("@/stores/appContext", () => ({
    useAppContextStore: () => ({ appSlug: "crm", version: "production" }),
}));

vi.mock("@/components/FormFieldRenderer.vue", () => ({
    default: { name: "FormFieldRenderer", template: "<div />" },
}));

const fields = [
    { name: "name", type: "string", display_name: "Name" },
    {
        name: "contacts",
        type: "relationship",
        related_collection: "contacts",
        relationship_type: "one_to_many",
        display_name: "Contacts",
    },
] as any[];

describe("FilterRuleComponent count operator", () => {
    it("renders a number input for a one-to-many count operator and emits the numeric value", async () => {
        const model: FilterCondition = {
            operator: "and",
            conditions: [
                { path: ["contacts"], operator: "count_gt", value: "" },
            ],
        };
        const wrapper = mount(FilterBuilder, {
            props: {
                modelValue: model,
                fields,
                collectionName: "customers",
            },
            global: {
                stubs: {
                    TreeSelect: true,
                    Select: true,
                    Button: true,
                    ToggleButton: true,
                    InputText: true,
                },
            },
        });

        await new Promise((r) => setTimeout(r, 10));

        const input = wrapper.find('input[type="number"]');
        expect(input.exists()).toBe(true);

        await input.setValue("5");

        const events = wrapper.emitted("update:modelValue");
        expect(events).toBeTruthy();
        const last = events![events!.length - 1][0] as any;
        expect(last.conditions[0].value).toBe(5);
    });

    it("renders a number input for an aggregate operator on a 1:M child field and emits the numeric value", async () => {
        const model: FilterCondition = {
            operator: "and",
            conditions: [
                { path: ["contacts", "score"], operator: "sum_gt", value: "" },
            ],
        };
        const wrapper = mount(FilterBuilder, {
            props: {
                modelValue: model,
                fields,
                collectionName: "customers",
            },
            global: {
                stubs: {
                    TreeSelect: true,
                    Select: true,
                    Button: true,
                    ToggleButton: true,
                    InputText: true,
                },
            },
        });

        await new Promise((r) => setTimeout(r, 10));

        const input = wrapper.find('input[type="number"]');
        expect(input.exists()).toBe(true);

        await input.setValue("1000");

        const events = wrapper.emitted("update:modelValue");
        expect(events).toBeTruthy();
        const last = events![events!.length - 1][0] as any;
        expect(last.conditions[0].operator).toBe("sum_gt");
        expect(last.conditions[0].value).toBe(1000);
    });
});

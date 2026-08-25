import { defineStore } from "pinia";
import { reactive, computed, Component, shallowRef } from "vue";
import type { FieldType } from "@/stores/collections";
import { ref } from "vue";

/** Registration for a plugin-provided input widget */
export interface InputWidgetRegistration {
    type: string;
    label: string;
    icon?: string;
    group?: string;
    custom?: boolean;
    supportedFieldTypes: FieldType[];
    preferredInputs?: string[];
    component: () => Promise<any>;
    settingsComponent?: () => Promise<any>;
    pluginSlug: string;
}

export interface ViewTypeRegistration {
    type: string;
    label: string;
    icon?: string;
    component: Component;
    settingsComponent?: Component;
    settings?: () => Promise<any>;
    pluginSlug: string;
}

export interface NavItemRegistration {
    label: string;
    icon: string;
    path: string;
    pluginSlug: string;
    sidebar: boolean;
    component: Component;
}

export const useExtensionRegistryStore = defineStore(
    "extensionRegistry",
    () => {
        const inputWidgetRegistry = ref<{
            [key: string]: InputWidgetRegistration;
        }>({});

        const displayWidgetRegistry = ref<{
            [key: string]: InputWidgetRegistration;
        }>({});

        const viewTypeRegistry = reactive(
            new Map<string, ViewTypeRegistration>(),
        );

        const navItemRegistry = ref<NavItemRegistration[]>([]);

        function registerInputWidget(
            registration: InputWidgetRegistration,
        ): void {
            inputWidgetRegistry.value[registration.type] = registration;
        }

        function registerDisplayWidget(
            registration: InputWidgetRegistration,
        ): void {
            displayWidgetRegistry.value[registration.type] = registration;
        }

        function registerViewType(registration: ViewTypeRegistration): void {
            viewTypeRegistry.set(registration.type, registration);
        }

        function registerNavItem(registration: NavItemRegistration): void {
            const findExistingIndex = navItemRegistry.value.findIndex(
                (e) =>
                    e.pluginSlug == registration.pluginSlug &&
                    e.path == registration.path,
            );

            if (findExistingIndex != -1) {
                navItemRegistry.value.splice(
                    findExistingIndex,
                    1,
                    registration,
                );
            } else {
                navItemRegistry.value.push(registration);
            }
        }

        function getRoutesForPlugin(pluginSlug: string) {
            return navItemRegistry.value.filter(
                (e) => e.pluginSlug == pluginSlug,
            );
        }

        function getInputWidget(
            type: string,
        ): InputWidgetRegistration | undefined {
            return inputWidgetRegistry.value[type];
        }

        function getView(type: string): ViewTypeRegistration | undefined {
            return viewTypeRegistry.get(type);
        }

        function getInputWidgetsForFieldType(
            fieldType: FieldType,
        ): InputWidgetRegistration[] {
            const results: InputWidgetRegistration[] = [];
            for (const name in inputWidgetRegistry.value) {
                const registration = inputWidgetRegistry.value[name];
                if (registration.supportedFieldTypes.includes(fieldType)) {
                    results.push(registration);
                }
            }
            return results;
        }

        function getDisplayWidget(
            type: string,
        ): InputWidgetRegistration | undefined {
            return displayWidgetRegistry.value[type];
        }

        function getDisplayWidgetsForFieldType(
            fieldType: FieldType,
        ): InputWidgetRegistration[] {
            const results: InputWidgetRegistration[] = [];
            for (const name in displayWidgetRegistry.value) {
                const registration = displayWidgetRegistry.value[name];
                if (registration.supportedFieldTypes.includes(fieldType)) {
                    results.push(registration);
                }
            }
            return results;
        }

        function unregisterPlugin(slug: string): void {
            for (const key in inputWidgetRegistry.value) {
                const reg = inputWidgetRegistry.value[key];
                if (reg.pluginSlug === slug)
                    delete inputWidgetRegistry.value[key];
            }
            for (const key in displayWidgetRegistry.value) {
                const reg = displayWidgetRegistry.value[key];
                if (reg.pluginSlug === slug)
                    delete displayWidgetRegistry.value[key];
            }
            for (const [key, reg] of viewTypeRegistry.entries()) {
                if (reg.pluginSlug === slug) viewTypeRegistry.delete(key);
            }

            navItemRegistry.value = navItemRegistry.value.filter(
                (e) => e.pluginSlug != slug,
            );
        }

        /** All registered view types (reactive, updated on registration/removal) */
        const allViewTypes = computed(() =>
            Array.from(viewTypeRegistry.values()),
        );

        return {
            registerInputWidget,
            registerDisplayWidget,
            registerViewType,
            getInputWidget,
            getDisplayWidget,
            getView,
            getInputWidgetsForFieldType,
            getDisplayWidgetsForFieldType,
            unregisterPlugin,
            allViewTypes,
            registerNavItem,
            getRoutesForPlugin,
            inputWidgetRegistry,
            displayWidgetRegistry,
        };
    },
);

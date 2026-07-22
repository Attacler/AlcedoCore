export interface Menu {
    id: string;
    name: string;
    icon: string;
    sections: MenuSection[];
}

export interface MenuItem {
    id: string;
    label: string;
    icon: string;
    visible: boolean;
    /** Internal vue-router path (e.g., "/collections") — mutually exclusive with url */
    route?: string;
    /** External URL (e.g., "https://docs.example.com") — mutually exclusive with route */
    url?: string;
    /** True when item links to an external URL (shows external link icon, opens new tab) per MENU-07 */
    external?: boolean;
    /** Link type: 'custom' | 'default' | 'plugin' | 'external'. Persisted user choice for the link type selector. */
    linkType?: string;
    /** Nested submenu items — collapsible tree display, up to 2 levels deep per MENU-04 */
    children?: MenuItem[];
}

export interface MenuSection {
    id: string;
    label: string;
    icon: string;
    visible: boolean;
    /** Items in this section */
    items: MenuItem[];
}

/** Helper to generate a unique ID for new sections and items */
export function generateMenuId(): string {
    return `menu_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
}

/** Create a new empty section with defaults */
export function createEmptySection(label?: string): MenuSection {
    return {
        id: generateMenuId(),
        label: label || "New Section",
        icon: "folder",
        visible: true,
        items: [],
    };
}

/** Create a new empty menu item with defaults */
export function createEmptyItem(label?: string): MenuItem {
    return {
        id: generateMenuId(),
        label: label || "New Item",
        icon: "link",
        visible: true,
    };
}

# Role-Based Menu System — Design Spec

## Problem

The current sidebar menu is a single global menu for all users. Every authenticated user sees the same sections and items regardless of their role. The `MenuItem` data model has no concept of scopes, roles, or permissions. Only the "Settings" mode toggle at the bottom of the sidebar is scope-gated. There is no way to define different navigation experiences for different roles.

## Solution Overview

Make menus first-class named entities that can be assigned to roles (many-to-many). Users with access to multiple menus can switch between them via a dropdown. Add a dedicated backend model with CRUD API, overhaul the MenuBuilder UI, and add a Menus overview page in Settings.

## Data Model

### Backend Tables

```sql
CREATE TABLE menus (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name VARCHAR(255) NOT NULL,
  icon VARCHAR(100) DEFAULT 'menu',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE menu_sections (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  menu_id UUID NOT NULL REFERENCES menus(id) ON DELETE CASCADE,
  label VARCHAR(255) NOT NULL,
  icon VARCHAR(100) DEFAULT '',
  visible BOOLEAN DEFAULT true,
  sort_order INTEGER DEFAULT 0
);

CREATE TABLE menu_items (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  section_id UUID NOT NULL REFERENCES menu_sections(id) ON DELETE CASCADE,
  parent_item_id UUID REFERENCES menu_items(id) ON DELETE CASCADE,
  label VARCHAR(255) NOT NULL,
  icon VARCHAR(100) DEFAULT '',
  visible BOOLEAN DEFAULT true,
  route VARCHAR(500),
  url VARCHAR(2000),
  external BOOLEAN DEFAULT false,
  link_type VARCHAR(50) DEFAULT 'custom',
  sort_order INTEGER DEFAULT 0
);

CREATE TABLE menu_roles (
  menu_id UUID NOT NULL REFERENCES menus(id) ON DELETE CASCADE,
  role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  PRIMARY KEY (menu_id, role_id)
);
```

### Nested structure (Rust + TypeScript)

The API returns and accepts a nested tree:

```typescript
interface Menu {
    id: string;
    name: string;
    icon: string;
    sections: MenuSection[];
}

interface MenuSection {
    id: string;
    label: string;
    icon: string;
    visible: boolean;
    sort_order: number;
    items: MenuItem[];
}

interface MenuItem {
    id: string;
    label: string;
    icon: string;
    visible: boolean;
    route?: string;
    url?: string;
    external?: boolean;
    link_type?: "custom" | "default" | "plugin" | "collection" | "external";
    sort_order: number;
    children?: MenuItem[];
}
```

### Which roles get which menus

A join table `menu_roles` links menus to roles. A role can have multiple menus. A menu can be assigned to multiple roles.

## API Endpoints

| Method   | Path                   | Scope Required       | Purpose                                                            |
| -------- | ---------------------- | -------------------- | ------------------------------------------------------------------ |
| `GET`    | `/api/menus`           | `settings.read.all`  | List all menus (name, icon, role count, item count)                |
| `POST`   | `/api/menus`           | `settings.write.all` | Create menu (name + icon only, empty sections)                     |
| `GET`    | `/api/menus/:id`       | `settings.read.all`  | Get full menu tree (sections + items + roles)                      |
| `PUT`    | `/api/menus/:id`       | `settings.write.all` | Full tree replace — replaces all sections/items inside the menu    |
| `DELETE` | `/api/menus/:id`       | `settings.write.all` | Delete menu and cascade sections/items                             |
| `GET`    | `/api/menus/:id/roles` | `settings.read.all`  | Get role IDs assigned to this menu                                 |
| `PUT`    | `/api/menus/:id/roles` | `settings.write.all` | Set role IDs assigned to this menu                                 |
| `POST`   | `/api/menus/:id/copy`  | `settings.write.all` | Clone sections/items from another menu by ID into this one         |
| `GET`    | `/api/menus/my`        | auth (any)           | Get all menus available to the current user (based on their roles) |

### Key endpoint details

**`PUT /api/menus/:id`** — accepts the full sections+items tree. The backend deletes all existing `menu_sections` and `menu_items` for this menu, then inserts the new ones. This avoids complex incremental CRUD. The frontend saves the entire tree on each save.

**`POST /api/menus/:id/copy`** — request body: `{ "source_menu_id": "uuid" }`. Deep-clones all sections and items from the source menu into this menu. The target menu must already exist (created via `POST /api/menus` first).

**`GET /api/menus/my`** — looks up the user's roles from their JWT/session, finds all menus linked to those roles via `menu_roles`, deduplicates, and returns the list. This is what the frontend calls on login to populate the menu switcher.

## Backend Implementation

### Rust files to create/modify

| File                                   | Action                                              |
| -------------------------------------- | --------------------------------------------------- |
| `alcedocore/src/db/menu.rs`            | New — DB query functions for menus, sections, items |
| `alcedocore/src/routes/admin/menus.rs` | New — API route handlers                            |
| `alcedocore/src/routes/admin/mod.rs`   | Modify — register menu routes                       |
| `alcedocore/src/db/mod.rs`             | Modify — export new module                          |
| `alcedocore/src/models/menu.rs`        | New — Rust structs matching DB schema               |
| `alcedocore/migrations/`               | New — SQL migration file                            |

### Scope checking

All menu management endpoints require `settings.read.all` or `settings.write.all` scopes, matching the existing pattern for Settings. The `GET /api/menus/my` endpoint requires only valid authentication (any scope or no scope).

### Migration from old settings

On first request to `GET /api/menus` after deployment, check if the old `menu_sections` app setting exists. If it does:

1. Parse the JSON
2. Create a new menu named "Default" with icon "menu"
3. Convert sections/items into the new DB schema
4. Assign this menu to all existing roles
5. Delete the old `menu_sections` setting key (or leave it — it won't be read)

This migration runs once. Future menu data comes from the new tables.

## Frontend Implementation

### Files to create/modify

| File                                                  | Action                                                                                                                                                |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `system-plugins/admin/src/types/menu.ts`              | **Modify** — add `Menu` type (name, icon, sections, id), add `roleIds` to Menu                                                                        |
| `system-plugins/admin/src/stores/menuStore.ts`        | **Rewrite** — load from `/api/menus/my` instead of app settings, track `menus[]` + `activeMenuId`, add menu switcher logic, keep dynamic item merging |
| `system-plugins/admin/src/stores/authStore.ts`        | **Modify** — after login, trigger menu loading                                                                                                        |
| `system-plugins/admin/src/components/AppLayout.vue`   | **Modify** — add menu switcher dropdown in sidebar header, keep Settings mode toggle at bottom                                                        |
| `system-plugins/admin/src/views/MenuBuilder.vue`      | **Overhaul** — add menu selector, name/icon editing, role assignment, copy, delete                                                                    |
| `system-plugins/admin/src/views/SettingsCategory.vue` | **Modify** — update reference to MenuBuilder                                                                                                          |
| `system-plugins/admin/src/views/MenusIndex.vue`       | **New** — dedicated Menus overview page in Settings                                                                                                   |
| `system-plugins/admin/src/router/index.ts`            | **Modify** — add `/settings/menus` route                                                                                                              |

### Menu loading flow

1. User logs in → `GET /api/auth/me` returns user info + roles
2. `menuStore.loadMyMenus()` → `GET /api/menus/my`
3. Response: `Menu[]` — each has id, name, icon, sections[], items[]
4. If `menus.length === 1` → `activeMenuId = menus[0].id`
5. If `menus.length > 1` → `activeMenuId` = last used (localStorage) or first
6. If `menus.length === 0` → show minimal sidebar (just the Settings button)

### Menu switcher (sidebar header)

In `AppLayout.vue`, add a dropdown at the top of the sidebar showing the current menu name + icon. When clicked, show all available menus. Selecting one sets `activeMenuId`. The sidebar reacts and re-renders with the new menu's sections/items.

The Settings mode toggle at the bottom remains exactly as-is — scope-gated, switching between Browse mode (active menu) and Settings mode (hardcoded settings sections).

### Dynamic items merging

At render time, the active menu's sections/items are the base. Then:

1. Plugin extension registry items (`registerNavItem()`) are inserted into matching sections by `sectionId`
2. Dynamic items (collections, plugin pages with `sidebar: true`) are inserted into matching sections

This logic stays in `menuStore.mergedSections` computed property, adapted to work on `activeMenu` instead of a single flat sections list.

### MenuBuilder UI redesign

```
┌──────────────────────────────────────────────┐
│  [Menu: ▼ Default]  [Copy From...] [Delete]  │
│                                               │
│  Name: [___________]   Icon: [icon picker]   │
│                                               │
│  Roles: [admin x] [editor x] [+ Add Role]    │
│                                               │
│  ── Sections ──                              │
│  ┌─ Content ─────────────────────────────┐   │
│  │  ◎ Dashboard                          │   │
│  │  ◎ Plugins                            │   │
│  │  ⚊ Registries                        │   │
│  │  └─ Sub Items ▼                       │   │
│  │    ◎ Reports                          │   │
│  │    ◎ Analytics                        │   │
│  └───────────────────────────────────────┘   │
│                                               │
│  [+ Add Section]                              │
│                                               │
│  [Save]                                       │
└──────────────────────────────────────────────┘
```

**New features in MenuBuilder:**

- **Menu selector dropdown** at top — switch between menus while editing
- **Name + icon editing** inline at the top
- **Role assignment** — autocomplete chip input that searches roles from the roles store. Adding/removing roles here auto-saves role assignments when the menu is saved
- **Copy from...** button → modal picker showing all menus → deep-clones sections/items into current menu (replaces existing)
- **Delete menu** button → confirmation dialog → deletes menu and returns to menu list

The existing drag-and-drop section/item editor stays largely the same but now targets the currently selected menu.

### Menus overview page (`MenusIndex.vue`)

New route at `/settings/menus` (linked from the Settings index page alongside General, Collections, Plugins).

Shows:

```
┌────────────────────────────────────────────────────────┐
│  Menus                                    [+ New Menu] │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ Name          Roles          Items    Actions   │   │
│  ├─────────────────────────────────────────────────┤   │
│  │ ☰ Default     admin, editor   12     Edit Copy ✕│   │
│  │ ☰ Viewer      viewer, public  4      Edit Copy ✕│   │
│  │ ☰ Minimal     sales           2      Edit Copy ✕│   │
│  └─────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────┘
```

- "Edit" → navigates to MenuBuilder for that menu
- "Copy" → creates a new menu with "Copy of {name}", deep-clones tree
- "✕" → delete with confirmation
- "+ New Menu" → creates new empty menu, navigates to MenuBuilder

### Settings mode bottom toggle

Kept exactly as-is in `AppLayout.vue`:

```vue
<button
    v-if="authStore.scopes.some(s => s === 'users.all' || s === 'settings.all' || ...)"
    @click="
        activeSection = activeSection === 'content' ? 'settings' : 'content'
    "
>
  {{ activeSection === 'content' ? '⚙ Settings' : '← Browse' }}
</button>
```

## Edge Cases

| Case                                       | Behavior                                                                                                                                                                             |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| User has no roles                          | Menu list is empty. Sidebar shows only the Settings button (if authorized) or an empty state.                                                                                        |
| User has 1 role with 0 menus               | Same as above.                                                                                                                                                                       |
| Admin creates 5 menus                      | Admin has `users.all` scope but that doesn't grant menu access. They need a role assigned to a menu, OR the admin role should be auto-assigned to the Default menu during migration. |
| Menu is deleted while users are viewing it | Frontend refreshes menu list on next navigation. Switcher shows remaining menus.                                                                                                     |
| Role is deleted                            | `ON DELETE CASCADE` cleans up `menu_roles` entries.                                                                                                                                  |
| Browser tab with old menu data stale       | No caching issue — every login fetches fresh menus.                                                                                                                                  |
| Dynamic items (collections) for menus      | Dynamic items merge into the active menu at render time. They apply to all menus equally.                                                                                            |

## Future Considerations (out of scope)

- Custom permissions per menu item (e.g., "only admins see this link within the editor menu")
- Per-user pinned/favorited menu items
- Menu items that link to external apps with SSO
- Drag-to-reorder sections within menu builder

## Spec Self-Review

- **Placeholders**: None. All sections are filled.
- **Internal consistency**: The backend model matches the API, which matches the frontend store. The menu switcher + Settings toggle don't conflict.
- **Scope check**: This is focused on role-based menus only. No scope creep into other areas.
- **Ambiguity check**: The many-to-many menu↔role relationship, tree-replace save strategy, and dynamic item merging are all explicitly defined.

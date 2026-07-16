# Card & Kanban View Formatting and Options

## Summary
Add template-based title formatting and configurable field selection to Cards and Kanban views. Each view owns its own settings panel, stored per-saved-view in `view_specific` config.

## Requirements
1. **Template formatting** for card/kanban titles using `{{field.name}}` syntax, with nested path support (`{{relation.field}}`)
2. **Field selection** — drag-to-reorder toggle list to choose which fields appear in the card body
3. **Settings panel** — each view renders its own gear button → PrimeVue Sidebar drawer
4. **Per-saved-view persistence** — stored in `SavedViewConfig.view_specific`
5. **Kanban extras** — same options as cards + `groupByField` selector moved into the drawer
6. **Missing fields in template** — render as empty string
7. **Fallback behavior** — when config is null/empty, use current defaults

## Architecture

```
View Component (CardsView / KanbanView)
├── Gear button (in-view toolbar)
└── SettingsDrawer (PrimeVue Sidebar, position="right")
    ├── TitleTemplate input (text field with placeholder)
    ├── FieldSelector (drag-to-reorder toggle list)
    └── GroupByField Select (kanban only)
```

### New files
| File | Purpose |
|------|---------|
| `src/utils/templateResolver.ts` | `resolveTemplate(template, item)` - parses `{{field}}` syntax |
| `src/components/SettingsDrawer.vue` | Reusable PrimeVue Sidebar wrapper |
| `src/components/FieldSelector.vue` | Drag-to-reorder toggle list for fields |

### Modified files
| File | Change |
|------|--------|
| `src/components/CardsView.vue` | Add gear button + settings drawer |
| `src/components/KanbanView.vue` | Add gear button + settings drawer, move group-by into drawer |

### Data model (`SavedViewConfig.view_specific`)
```typescript
{
  titleField: string,           // existing
  groupByField: string,         // existing (kanban)
  titleTemplate: string | null, // new — e.g. "{{number}} - {{name}}"
  cardFields: string[] | null   // new — ordered visible field names
}
```
- `null` = use defaults (current behavior)
- `titleTemplate` takes priority over `titleField` when set

### Template resolver
```typescript
resolveTemplate("{{number}} - {{customer.name}}", item)
// → "42 - Acme Corp"
```
- Regex: `/\{\{\s*([^}]+)\s*\}\}/g`
- Supports nested paths via `.` split-and-traverse
- Missing/null fields → empty string
- No valid matches → empty string → fallback to "(untitled)"

### FieldSelector component
- Props: `fields: FieldDefinition[]`, `modelValue: string[]`
- Each field row: drag handle (⋮⋮), toggle switch, field name, type badge
- Non-selected fields at bottom (toggled off), drag only for ordering selected ones
- Emits `update:modelValue` with new ordered list

### Settings persistence
- Changes saved via `savedViewsStore.updateView()` (API PUT)
- If no active saved view, settings stored in local ref; user prompted to save view

### Edge cases
- Empty template → use titleField default
- Template referencing unknown field → empty string
- No fields toggled → show "No fields selected" in card body
- null/undefined field values in template → empty string
- Drag reorder on empty → all fields shown toggled-off by default

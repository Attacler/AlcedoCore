# Collection Builder — Field Sections Design

## Summary

Add named, ordered sections to the collection builder that group fields into a 2-column layout. Sections and relational sections share a single ordered list — they can be freely interleaved.

## Motivation

The current collection builder shows all fields in a flat 2-column grid with no grouping. As collections grow (10+ fields), this becomes hard to navigate. Named sections let users organize fields into logical groups (e.g., "Basic Info", "Pricing", "SEO") with full control over ordering.

## Design

### Data Model

The `collection_sections` table is extended:

```sql
ALTER TABLE collection_sections 
  ADD COLUMN section_type TEXT NOT NULL DEFAULT 'relational',
  ALTER COLUMN relation_field DROP NOT NULL;
```

Two section types:
- **`field_group`** — groups fields together in a named section. `name` is the section label; `display_fields` (TEXT[]) stores the ordered field names belonging to this section.
- **`relational`** — existing behavior: renders related records via `relation_field`. `relation_field` becomes nullable and only required for this type.

Fields stay flat in `collection_definitions.fields` JSONB (each field has its own `ordinal_position` used for ordering within its section).

### Unified Section Ordering

All sections share the `ordinal_position` column. The order of field_group and relational sections is interleaved — there is no separate "Relational Sections" panel.

### Migration

When GET `/collections/:name/sections` returns zero sections, the backend auto-creates a single field_group section named "Fields" containing all existing field names in their current order.

### Backend API

No new endpoints. Existing section CRUD extended:
- `POST /collections/:name/sections` accepts `section_type` and nullable `relation_field`
- `PUT /collections/:name/sections/:id` same
- `GET /collections/:name/sections` returns `section_type` in payload
- `DELETE /collections/:name/sections/:id` unchanged

### Frontend — CollectionBuilder.vue

**Form preview** becomes section-organized:

```
┌─ Section: "Basic Info" ───────────────┐
│ ┌─ name ───────┐ ┌─ description ────┐ │
│ └──────────────┘ └──────────────────┘ │
│ ┌─ status (full width) ──────────────┐ │
│ └────────────────────────────────────┘ │
├─ Section: "Orders" (relational) ──────┤
│ via orders → order_items, table view  │
├─ Section: "Pricing" ──────────────────┤
│ ┌─ price ──────┐ ┌─ tax_rate ───────┐ │
│ └──────────────┘ └──────────────────┘ │
└────────────────────────────────────────┘
```

- Sections render in `ordinal_position` order
- Each section has a drag handle for reordering
- Section header shows name + type icon + edit/delete buttons
- `field_group` sections: 2-column grid with `col-span-2` for full-width fields
- `relational` sections: compact info row (relation field, view type)
- "Add Section" dialog offers two types: Field Group or Relational
- Section editor adapts form based on type
- Fields can be dragged between sections; palette drops into hovered section
- The separate "Relational Sections" panel at the bottom is removed

### Frontend — TypeScript Types

```typescript
interface CollectionSection {
  id?: string
  name: string
  section_type: 'field_group' | 'relational'
  relation_field?: string | null
  view_type?: string
  default_filter?: any
  display_fields?: string[] | null
  item_limit?: number
  ordinal_position: number
}
```

### RecordDetail.vue

If sections exist, fields render in their section groups with section headers. Relational sections render at their ordinal position. Falls back to current flat rendering when no sections exist.

## Files Changed

| File | Change |
|------|--------|
| `plugin-core/db-init/013-collection-field-sections.sql` | New migration |
| `plugin-core/src/api/collections.rs` | Section handlers: add section_type, nullable relation_field, auto-migration |
| `plugin-core/src/db/collections.rs` | Section types, auto-create default section |
| `system-plugins/admin/src/stores/collections.ts` | Update interfaces, API helpers |
| `system-plugins/admin/src/views/CollectionBuilder.vue` | Section-organized form preview, unified section list, types |
| `system-plugins/admin/src/views/RecordDetail.vue` | Section-aware field rendering |

## Open Questions

Resolved during brainstorming:
- All fields must belong to a section
- Existing collections auto-migrate to a single default section on first load

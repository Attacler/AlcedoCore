# Report: physical schema sync gaps in version → production deploy

Status: analysis + **the missing-FK gap is now fixed** (see "Fixed" below).
Scope: what happens when a **field-level change** made in a sandbox version is
deployed to production, and whether the remaining gaps must be fixed.

## Background: what a deploy copies

A deploy (`POST /api/platform/versions/{id}/deploy`) copies, per app:

- all `alcedocore_*` **metadata** tables (copy set) — collections, fields, layouts,
  views, menus, settings, file metadata, saved views, migrator state;
- **structure-only** clones of user tables that are new in the version
  (`CREATE TABLE LIKE … INCLUDING ALL` — note: `LIKE` never copies FK constraints);
- **additive column sync** for user tables that already exist in production
  (`missing_user_columns_sql`: `ALTER TABLE … ADD COLUMN IF NOT EXISTS`);
- FK constraints are recreated **only** for copy-set tables and newly created tables
  (`overwrite_schema_batched_tx` / `recreate_schema_foreign_keys`);
- plugin catalog/installs (with runtime provisioning) and route mappings.

Permissions, logs, developer keys, KV and user-table **rows** are never copied.

## The limitation, precisely

Three field-level change types are **not** physically applied to existing
production tables:

| Change made in version | Metadata row in production | Physical column | Physical FK |
| --- | --- | --- | --- |
| New field on an **existing** collection | ✅ copied | ✅ `ADD COLUMN IF NOT EXISTS` | ✅ **FIXED** — synced via `missing_user_fks`/`apply_missing_user_fks` (remapped, NOT VALID fallback) |
| New field on a **new** collection | ✅ copied | ✅ (via `LIKE`) | ✅ recreated (remapped to `{app}010production`, incl. cross-app `related_app` refs) |
| Field **type change** on existing collection | ⛔ **deploy refused up front** (HTTP 409, `unsupported: [{type: "field_type_change"}]`) | — | — |
| Field **deleted** on existing collection | ⛔ **deploy refused up front** (HTTP 409, `unsupported: [{type: "field_deleted"}]`) | — | — |

(Relation fields added to a new collection are fully handled since the FK
recreation fix; the gap is only for tables that already exist in production.)

## Impact analysis

**API level: no functional gap.** Relations are resolved at query time from
metadata (`alcedocore_fields.related_app` / `related_collection` →
`relation_target()` in `collections/ddl.rs`), not from the physical FK. Reads
(`fields=rel.*`), writes and deletes of relation fields work in production after
deploy; item deletes are handled in the service layer, and m2o FKs use
`ON DELETE RESTRICT` anyway (no cascade reliance). Verified E2E: after deploying a
cross-app relation field, `GET /api/app/items/...?fields=shop_customer.id` works.

**DB level: integrity gap.** Without the physical FK on an existing production
table:

- nothing at the DB level prevents orphan rows (only reachable via direct SQL
  access — the API always writes through the items service);
- `ON DELETE RESTRICT` is not enforced for direct DB writes;
- the gap propagates: the next version cloned from production inherits the
  missing FK (clone recreates FKs from the source schema's definitions, and the
  source lacks it).

**Type changes / deletions** are a different class: applying them automatically
means `ALTER COLUMN TYPE … USING …` (can fail on existing data, needs a cast
strategy) or `DROP COLUMN` (irreversible data loss). A deploy runs in one
transaction per app, so a failing `ALTER TYPE` would roll back the whole app's
deploy.

## Verdict: should vs must

| Gap | Verdict | Reasoning |
| --- | --- | --- |
| Missing FK for a new relation field on an existing table | **Should fix** → **FIXED** | API works without it; the FK is defense-in-depth for DB integrity. Fixed as sketched below; verified by unit tests + E2E in docker compose. |
| Field type change not applied | **Must NOT auto-apply** → **now refused up front** | Data conversion is data-dependent (`USING` casts, lossy narrowing). Instead of silently deploying metadata-only, the deploy is refused before any write: `check_unsupported_field_changes` compares each field edit's version row against the production row (keyed by `collection_id:api_name`, comparing the physical signature `type`/`relationship_type`/`related_collection`/`related_app` from `options`) and returns HTTP 409 + JSend `fail` with `unsupported: [{type: "field_type_change", detail: …}]` (also reported in `dry_run`). Nothing is written. |
| Field deletion not applied (column/FK kept) | **Must NOT auto-apply** → **now refused up front** | `DROP COLUMN` is irreversible. Same mechanism: a field row present in production but gone in the version yields `unsupported: [{type: "field_deleted", …}]` and the deploy is refused with 409 before any write. |

Bottom line: nothing here breaks the API contract of a deploy, so none of it is a
"must fix" for correctness of the product. The missing-FK case was the only one
worth fixing proactively because it is safe, cheap, and closes a real (if
narrow) integrity hole that otherwise propagates into every future clone.

## Implementation (done — the "should fix" item)

In `core/src/services/versions/clone_production.rs`:

1. `missing_user_fks(conn, source, target, only_tables)` — queries `pg_constraint`
   for FKs on source **user tables** that exist in the target and whose constraint
   name is not yet present there (idempotent); `only_tables` restricts to given
   tables (selected mode), `None` = all user tables (wholesale).
2. `apply_missing_user_fks(conn, source, target, fks)` — remaps each definition
   with `remap_fk_schema` (same-app and cross-app `related_app` refs alike), skips
   FKs whose remapped referenced table doesn't exist (warns), and adds each via
   `add_fk_not_valid_fallback`.
3. `add_fk_not_valid_fallback(conn, add_sql)` — **savepoint-based** try-VALID /
   fallback-NOT-VALID. A failed statement aborts a Postgres transaction (25P02),
   so the attempt runs inside `SAVEPOINT alcedo_add_fk` and is rolled back to it
   on error; without the savepoint the fallback could never execute. (This also
   fixed the pre-existing cross-FK loop in `overwrite_schema_batched_tx`, which
   used the broken bare try/fallback pattern inside the deploy transaction.)
4. `sync_missing_user_fks(...)` — query + apply convenience for selected mode.

Wiring:

- Wholesale (`overwrite_schema_batched_tx`): FKs are **queried before** the main
  batch (so tables created by the batch itself are excluded — they get FKs from
  the copy-set `source_fks` recreation) and **applied after** it (the FK's column
  is added by the batch's column sync).
- Selected (`apply_selected_edits` in `deploy/apply.rs`): after the column sync,
  FKs are synced for the collections with field edits (same `field_tables` list)
  plus collections created by selected `collection_created` edits (their table
  was just created via `LIKE`, which never copies FKs).
- Removed fields are deliberately **not** dropped (see verdict table).

## Verification (done)

- Unit (DB-backed, `deploy/tests.rs`, 25/25 green): wholesale deploy adds the FK
  to the production user table remapped to `{app}010production`; selected mode
  (`edit_ids`) does the same; an orphan production row makes the deploy succeed
  with the FK `NOT VALID` (`convalidated = false`); cross-app relation field on
  an existing collection deploys an FK referencing `{other}010production`;
  a field type change (string→int) and a field deletion in a version are each
  refused with `ok:false` + `unsupported` (wholesale, selected and dry_run),
  leaving production untouched.
- E2E (docker compose + curl, release image): added `e2e_main_contact` (m2o →
  customers) to `contacts` in version `e2efk`, wholesale deploy →
  `crm010production.contacts` got `FOREIGN KEY (e2e_main_contact) REFERENCES
  crm010production.customers(id)` (validated), and `POST /api/app/items/contacts`
  with the new field succeeded in production. Selected mode with `edit_ids=[2]`
  for a second field produced the same result. Type change of `email`
  (string→int) and deletion of `email` in sandbox versions were each refused
  with HTTP 409 + `unsupported: [{type: "field_type_change"/"field_deleted"}]`,
  `deployed: false`, production unchanged. All E2E artifacts were removed
  afterwards.

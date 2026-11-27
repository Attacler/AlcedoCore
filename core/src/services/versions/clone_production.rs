use sqlx::{Acquire, Executor, PgConnection, PgPool};
use std::error::Error as StdError;
use std::time::Instant;
use tokio::time::{Duration, sleep};

const BATCH_SIZE: usize = 10_000; // rows per batch
const BATCH_DELAY_MS: u64 = 50; // pause between batches to ease DB pressure

/// Tables that are cloned structure-only (never data-copied): user
/// permissions and activity logs. Every other `alcedocore_*` table plus
/// `_sqlx_migrator_migrations` makes up the copy set.
pub const EXCLUDED_COPY_TABLES: &[&str] = &[
    "alcedocore_roles",
    "alcedocore_role_scopes",
    "alcedocore_role_policies",
    "alcedocore_policies",
    "alcedocore_policy_permissions",
    "alcedocore_user_roles",
    // Permission data: role bindings of layouts/menus reference
    // `alcedocore_roles(id)`, which is excluded — version role UUIDs do not
    // exist in production, so these tables must not be data-copied either.
    "alcedocore_collection_layout_roles",
    "alcedocore_menu_roles",
    // Plugin→policy bindings reference `alcedocore_policies(id)` (excluded,
    // same reason): copied rows would dangle in the target.
    "alcedocore_plugin_policies",
    "alcedocore_system_logs",
    "alcedocore_collection_logs",
];

/// Quotes an SQL identifier (defense in depth: schema/table names are built
/// from slugs, but never interpolate them raw).
pub fn qident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Quotes a schema-qualified table name.
pub fn qtable(schema: &str, table: &str) -> String {
    format!("{}.{}", qident(schema), qident(table))
}

/// Rewrites the schema references in an FK definition from the source
/// version's schemas to the target version's. Same-app references
/// (`source_schema.`) and cross-app references (`{other_app}010{src_ver}.`,
/// created by `related_app` relation fields) both become
/// `{...}010{tgt_ver}.` — without this, a deployed table's FK would keep
/// pointing at the version schema of the other app.
pub fn remap_fk_schema(fk_def: &str, source_schema: &str, target_schema: &str) -> String {
    if source_schema.starts_with("alcedocore_") {
        return fk_def.to_string();
    }
    let def = fk_def.replace(
        &format!("{}.", source_schema),
        &format!("{}.", target_schema),
    );
    if let (Some((_, src_ver)), Some((_, tgt_ver))) = (
        source_schema.split_once("010"),
        target_schema.split_once("010"),
    ) {
        if src_ver != tgt_ver {
            return def.replace(
                &format!("010{}.", src_ver),
                &format!("010{}.", tgt_ver),
            );
        }
    }
    def
}

/// Quotes an SQL string literal.
fn qlit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Executes a multi-statement SQL string in ONE round trip (simple protocol).
/// Outside an explicit transaction the statements run in a single implicit
/// transaction (all-or-nothing); inside one they run within the caller's tx.
/// The per-table round trips this replaces dominated clone/deploy time.
///
/// Non-async on purpose: calling `raw_sql(...).execute(&mut *conn)` inside an
/// async fn hits sqlx's "implementation of `Executor` is not general enough"
/// HRTB error; unifying both lifetimes in one signature avoids it.
pub(crate) fn exec_multi<'a>(
    conn: &'a mut PgConnection,
    sql: &'a str,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<sqlx::postgres::PgQueryResult, sqlx::Error>> + Send + 'a>,
> {
    Box::pin(conn.execute(sqlx::raw_sql(sql)))
}

/// Serial columns of a whole schema: (table, column, source sequence name).
async fn serial_cols_schema(
    conn: &mut PgConnection,
    schema: &str,
) -> Result<Vec<(String, String, String)>, Box<dyn StdError + Send + Sync>> {
    Ok(sqlx::query_as(
        "SELECT table_name, column_name, substring(column_default from 'nextval\\(''([^'']+)''') \
         FROM information_schema.columns \
         WHERE table_schema = $1 AND column_default LIKE 'nextval%'",
    )
    .bind(schema)
    .fetch_all(&mut *conn)
    .await?)
}

/// Identity columns of a whole schema: (table, column).
async fn identity_cols_schema(
    conn: &mut PgConnection,
    schema: &str,
) -> Result<Vec<(String, String)>, Box<dyn StdError + Send + Sync>> {
    Ok(sqlx::query_as(
        "SELECT table_name, column_name FROM information_schema.columns \
         WHERE table_schema = $1 AND is_identity = 'YES'",
    )
    .bind(schema)
    .fetch_all(&mut *conn)
    .await?)
}

/// Builds the sequence-fix statements for a whole schema — same semantics as
/// the per-table [`fix_table_sequences`], but as one batch of SQL text.
fn sequence_fix_sql(
    source_schema: &str,
    target_schema: &str,
    serial_cols: &[(String, String, String)],
    identity_cols: &[(String, String)],
) -> String {
    let mut sql = String::new();
    for (table, col, seq) in serial_cols {
        let source_prefix = format!("{}.", source_schema);
        let target_seq = if let Some(rest) = seq.strip_prefix(&source_prefix) {
            format!("{}.{}", target_schema, rest)
        } else if seq.contains('.') {
            seq.clone()
        } else {
            format!("{}.{}", target_schema, seq)
        };
        sql.push_str(&format!(
            "CREATE SEQUENCE IF NOT EXISTS {seq};\n\
             ALTER TABLE {tbl} ALTER COLUMN {col} SET DEFAULT nextval({seq_lit}::regclass);\n\
             ALTER SEQUENCE {seq} OWNED BY {tbl}.{col};\n\
             SELECT setval({seq_lit}, COALESCE((SELECT MAX({col}) FROM {tbl}), 0) + 1, false);\n",
            seq = qseq(&target_seq),
            tbl = qtable(target_schema, table),
            col = qident(col),
            seq_lit = qlit(&target_seq),
        ));
    }
    for (table, col) in identity_cols {
        sql.push_str(&format!(
            "SELECT setval(pg_get_serial_sequence({tbl_lit}, {col_lit}), \
             COALESCE((SELECT MAX({col}) FROM {tbl}), 0) + 1, false);\n",
            tbl_lit = qlit(&qtable(target_schema, table)),
            col_lit = qlit(col),
            col = qident(col),
            tbl = qtable(target_schema, table),
        ));
    }
    sql
}

/// Rewrites `app_name`/`app_version` in the target schema's
/// `alcedocore_collections` to the target schema's own app/version. Copied
/// rows carry the source schema's values, but the meta cache matches rows by
/// `{app}010{version}` (see `SchemaService::refresh_meta_into`), so without
/// this the copied collections would have no meta in the target schema.
pub async fn rewrite_collection_app_columns(
    conn: &mut PgConnection,
    target_schema: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let Some((app, version)) = target_schema.split_once("010") else {
        return Ok(());
    };
    let table = qtable(target_schema, "alcedocore_collections");
    // One round trip: guard on the table existing inside the DO block.
    exec_multi(
        conn,
        &format!(
            "DO $$ BEGIN \
               IF to_regclass({tbl_lit}) IS NOT NULL THEN \
                 UPDATE {tbl} SET app_name = {app}, app_version = {ver}; \
               END IF; \
             END $$;",
            tbl_lit = qlit(&table),
            tbl = table,
            app = qlit(app),
            ver = qlit(version),
        ),
    )
    .await?;
    Ok(())
}

/// The copy set of a schema: all `alcedocore_*` tables except
/// [`EXCLUDED_COPY_TABLES`], plus `_sqlx_migrator_migrations`. User tables
/// (non-`alcedocore_*`) are not part of the copy set — they are cloned
/// structure-only.
pub async fn copy_set_tables(
    pool: &PgPool,
    schema: &str,
) -> Result<Vec<String>, Box<dyn StdError + Send + Sync>> {
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = $1 AND table_type = 'BASE TABLE'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;
    Ok(tables
        .into_iter()
        .map(|(name,)| name)
        .filter(|name| {
            name == "_sqlx_migrator_migrations"
                || (name.starts_with("alcedocore_") && !EXCLUDED_COPY_TABLES.contains(&name.as_str()))
        })
        .collect())
}


/// `(collection table, field api_name) -> options type` for one schema's field
/// metadata, used to validate column repairs against the metadata.
async fn field_type_map(
    conn: &mut PgConnection,
    schema: &str,
) -> Result<std::collections::HashMap<(String, String), String>, Box<dyn StdError + Send + Sync>>
{
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT c.\"table\", f.api_name, f.options->>'type' \
         FROM {} f JOIN {} c ON c.id = f.collection_id",
        qtable(schema, "alcedocore_fields"),
        qtable(schema, "alcedocore_collections"),
    ))
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(t, c, ty)| ((t, c), ty.unwrap_or_default()))
        .collect())
}

/// `ALTER TABLE ... ADD COLUMN` statements for every column that exists in a
/// source user table but not in the same-named target table.
///
/// Field edits only copy the `alcedocore_fields` metadata row — the physical
/// column must be added to the target's user table as well, or the items API
/// rejects writes with "Unknown field". `only_tables` restricts the sync to
/// the given user tables (selected mode: only collections with field edits);
/// `None` syncs all user tables (wholesale mode). Columns that exist only in
/// the target are never dropped — that would lose production data.
pub async fn missing_user_columns_sql(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    only_tables: Option<&[String]>,
) -> Result<String, Box<dyn StdError + Send + Sync>> {
    let rows: Vec<(String, String, String, bool, Option<String>)> = sqlx::query_as(
        "SELECT src.relname, a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                pg_get_expr(d.adbin, d.adrelid) \
         FROM pg_attribute a \
         JOIN pg_class src ON src.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = src.relnamespace \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE n.nspname = $1 AND src.relkind = 'r' \
           AND src.relname NOT LIKE 'alcedocore\\_%' \
           AND src.relname <> '_sqlx_migrator_migrations' \
           AND ($3::text[] IS NULL OR src.relname = ANY($3)) \
           AND a.attnum > 0 AND NOT a.attisdropped \
           AND EXISTS ( \
                SELECT 1 FROM pg_class tc \
                JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
                WHERE tn.nspname = $2 AND tc.relname = src.relname AND tc.relkind = 'r') \
           AND NOT EXISTS ( \
                SELECT 1 FROM pg_attribute t \
                JOIN pg_class tc ON tc.oid = t.attrelid \
                JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
                WHERE tn.nspname = $2 AND tc.relname = src.relname AND t.attname = a.attname \
                  AND NOT t.attisdropped) \
         ORDER BY src.relname, a.attnum",
    )
    .bind(source_schema)
    .bind(target_schema)
    .bind(only_tables.map(|t| t.to_vec()))
    .fetch_all(&mut *conn)
    .await?;
    // Validate repair candidates against the field metadata of BOTH schemas:
    // an unlogged version-side type change must not create a physical column
    // whose type contradicts production's own metadata. Repairs are kept when
    // the definitions agree (or when either side has no metadata row).
    let src_meta = field_type_map(&mut *conn, source_schema).await?;
    let tgt_meta = field_type_map(&mut *conn, target_schema).await?;
    let mut sql = String::new();
    for (table, col, ty, not_null, default) in rows {
        if let (Some(vt), Some(pt)) = (
            src_meta.get(&(table.clone(), col.clone())),
            tgt_meta.get(&(table.clone(), col.clone())),
        ) {
            if vt != pt {
                tracing::warn!(
                    "deploy: skipping column repair {}.{}: version field type '{vt}' contradicts production metadata '{pt}'",
                    table,
                    col
                );
                continue;
            }
        }
        sql.push_str(&format!(
            "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
            qtable(target_schema, &table),
            qident(&col),
            ty
        ));
        if let Some(default) = default {
            sql.push_str(&format!(" DEFAULT {}", default));
        }
        if not_null {
            sql.push_str(" NOT NULL");
        }
        sql.push_str(";\n");
    }
    Ok(sql)
}

/// Foreign keys that exist on a source **user table** but not on the
/// same-named target table: `(con table, constraint name, constraint def,
/// referenced schema, referenced table)`. `only_tables` restricts the sync to
/// the given user tables (selected mode: only collections with field/creation
/// edits); `None` syncs all user tables (wholesale mode). Tables that only
/// exist in the source are excluded — they are created structure-only and get
/// their FKs from the copy-set FK recreation instead. Constraints already
/// present in the target (same name on the same table) are skipped, so
/// re-running is idempotent.
pub async fn missing_user_fks(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    only_tables: Option<&[String]>,
) -> Result<Vec<(String, String, String, String, String)>, Box<dyn StdError + Send + Sync>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT cl.relname, c.conname, pg_get_constraintdef(c.oid), rn.nspname, rl.relname \
         FROM pg_constraint c \
         JOIN pg_class cl ON cl.oid = c.conrelid \
         JOIN pg_namespace n ON n.oid = cl.relnamespace \
         JOIN pg_class rl ON rl.oid = c.confrelid \
         JOIN pg_namespace rn ON rn.oid = rl.relnamespace \
         WHERE c.contype = 'f' AND n.nspname = $1 \
           AND cl.relname NOT LIKE 'alcedocore\\_%' \
           AND cl.relname <> '_sqlx_migrator_migrations' \
           AND ($3::text[] IS NULL OR cl.relname = ANY($3)) \
           AND EXISTS ( \
                SELECT 1 FROM pg_class tc \
                JOIN pg_namespace tn ON tn.oid = tc.relnamespace \
                WHERE tn.nspname = $2 AND tc.relname = cl.relname AND tc.relkind = 'r') \
           AND NOT EXISTS ( \
                SELECT 1 FROM pg_constraint tc \
                JOIN pg_class tcl ON tcl.oid = tc.conrelid \
                JOIN pg_namespace tn ON tn.oid = tcl.relnamespace \
                WHERE tn.nspname = $2 AND tcl.relname = cl.relname AND tc.conname = c.conname) \
         ORDER BY cl.relname, c.conname",
    )
    .bind(source_schema)
    .bind(target_schema)
    .bind(only_tables.map(|t| t.to_vec()))
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// Adds one FK constraint inside the caller's transaction, falling back to
/// `NOT VALID` when existing target rows would violate it. A failed statement
/// aborts a Postgres transaction (25P02), so the attempt runs inside a
/// SAVEPOINT that is rolled back on error — without that, the fallback could
/// never execute. If `add_sql` already ends in `NOT VALID` the fallback is a
/// no-op retry of the same statement.
pub async fn add_fk_not_valid_fallback(
    conn: &mut PgConnection,
    add_sql: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    sqlx::query("SAVEPOINT alcedo_add_fk")
        .execute(&mut *conn)
        .await?;
    if sqlx::query(add_sql).execute(&mut *conn).await.is_err() {
        sqlx::query("ROLLBACK TO SAVEPOINT alcedo_add_fk")
            .execute(&mut *conn)
            .await?;
        let fallback = if add_sql.trim_end().ends_with("NOT VALID") {
            add_sql.to_string()
        } else {
            format!("{} NOT VALID", add_sql)
        };
        sqlx::query(&fallback).execute(&mut *conn).await?;
    }
    sqlx::query("RELEASE SAVEPOINT alcedo_add_fk")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Applies FKs returned by [`missing_user_fks`] to the target schema: each
/// definition's schema references are remapped to the target (same-app and
/// cross-app `related_app` references alike) and the constraint is added with
/// the NOT VALID fallback. FKs whose remapped referenced table does not exist
/// in the target are skipped with a warning (the referenced app may not be
/// linked to the target version at all). Returns how many FKs were added.
pub async fn apply_missing_user_fks(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    fks: Vec<(String, String, String, String, String)>,
) -> Result<usize, Box<dyn StdError + Send + Sync>> {
    let mut added = 0;
    for (table, _conname, def, ref_schema, ref_table) in fks {
        let def = remap_fk_schema(&def, source_schema, target_schema);
        // Remap the referenced schema the same way for the existence check.
        let ref_schema_mapped =
            remap_fk_schema(&format!("{}.", ref_schema), source_schema, target_schema);
        let ref_q = format!(
            "{}.{}",
            qident(ref_schema_mapped.trim_end_matches('.')),
            qident(&ref_table)
        );
        let (exists,): (bool,) = sqlx::query_as("SELECT to_regclass($1) IS NOT NULL")
            .bind(&ref_q)
            .fetch_one(&mut *conn)
            .await?;
        if !exists {
            tracing::warn!(
                "deploy: skipping FK on {}.{} — referenced table {} does not exist",
                target_schema,
                table,
                ref_q
            );
            continue;
        }
        let add = format!("ALTER TABLE {} ADD {}", qtable(target_schema, &table), def);
        add_fk_not_valid_fallback(conn, &add).await?;
        added += 1;
    }
    Ok(added)
}

/// Convenience: query [`missing_user_fks`] and apply them in one call. Use
/// when query and apply happen at the same point (selected-mode deploy, inside
/// the caller's transaction); the wholesale overwrite queries up front (so
/// not-yet-created tables are excluded) and applies after its main batch.
pub async fn sync_missing_user_fks(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    only_tables: Option<&[String]>,
) -> Result<usize, Box<dyn StdError + Send + Sync>> {
    let fks = missing_user_fks(conn, source_schema, target_schema, only_tables).await?;
    apply_missing_user_fks(conn, source_schema, target_schema, fks).await
}

/// Tables + serial columns + identity columns of a schema in ONE round trip:
/// rows are (kind, a, b, c) with kind 't' (table), 's' (serial: table, col, seq),
/// 'i' (identity: table, col).
async fn schema_meta_conn(
    conn: &mut PgConnection,
    schema: &str,
) -> Result<(Vec<String>, Vec<(String, String, String)>, Vec<(String, String)>), Box<dyn StdError + Send + Sync>> {
    let rows: Vec<(String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT 't'::text AS kind, t.table_name AS a, NULL::text AS b, NULL::text AS c \
         FROM information_schema.tables t WHERE t.table_schema = $1 AND t.table_type = 'BASE TABLE' \
         UNION ALL \
         SELECT 's', c.table_name, c.column_name, substring(c.column_default from 'nextval\\(''([^'']+)''') \
         FROM information_schema.columns c WHERE c.table_schema = $1 AND c.column_default LIKE 'nextval%' \
         UNION ALL \
         SELECT 'i', c.table_name, c.column_name, NULL \
         FROM information_schema.columns c WHERE c.table_schema = $1 AND c.is_identity = 'YES' \
         ORDER BY 1, 2, 3",
    )
    .bind(schema)
    .fetch_all(&mut *conn)
    .await?;
    let mut tables = Vec::new();
    let mut serial = Vec::new();
    let mut identity = Vec::new();
    for (kind, a, b, c) in rows {
        match kind.as_str() {
            "t" => tables.push(a.unwrap_or_default()),
            "s" => serial.push((a.unwrap_or_default(), b.unwrap_or_default(), c.unwrap_or_default())),
            "i" => identity.push((a.unwrap_or_default(), b.unwrap_or_default())),
            _ => {}
        }
    }
    Ok((tables, serial, identity))
}

async fn schema_tables_conn(
    conn: &mut PgConnection,
    schema: &str,
) -> Result<Vec<String>, Box<dyn StdError + Send + Sync>> {
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' ORDER BY table_name",
    )
    .bind(schema)
    .fetch_all(&mut *conn)
    .await?;
    Ok(tables.into_iter().map(|(name,)| name).collect())
}

/// FK definitions of the given tables of `schema` in ONE catalog query:
/// (con table, constraint def, referenced table). Shared by the wholesale
/// overwrite and the clone-path FK recreation, so neither loops per table
/// with its own SELECT.
async fn source_schema_fks(
    conn: &mut PgConnection,
    schema: &str,
    tables: &[String],
) -> Result<Vec<(String, String, String)>, Box<dyn StdError + Send + Sync>> {
    Ok(sqlx::query_as(
        "SELECT cl.relname, pg_get_constraintdef(c.oid), rl.relname \
         FROM pg_constraint c \
         JOIN pg_class cl ON cl.oid = c.conrelid \
         JOIN pg_class rl ON rl.oid = c.confrelid \
         JOIN pg_namespace n ON n.oid = cl.relnamespace \
         WHERE c.contype = 'f' AND n.nspname = $1 AND cl.relname = ANY($2)",
    )
    .bind(schema)
    .bind(tables)
    .fetch_all(&mut *conn)
    .await?)
}

/// Inserts one batch of rows of `table_name` from source to target on the
/// given connection. The caller manages triggers and the transaction.
async fn copy_one_batch(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
    limit: i64,
    offset: i64,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    sqlx::query(&format!(
        "INSERT INTO {target} OVERRIDING SYSTEM VALUE
         SELECT * FROM {source}
         ORDER BY 1  -- stable order for paging
         LIMIT {limit} OFFSET {offset}",
        target = qtable(target_schema, table_name),
        source = qtable(source_schema, table_name),
        limit = limit,
        offset = offset,
    ))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Clone path: copies all rows of one table from `source_schema` to
/// `target_schema` in per-batch transactions, with triggers disabled on the
/// target. `OVERRIDING SYSTEM VALUE` is required for GENERATED ALWAYS identity
/// columns and is a no-op otherwise.
pub async fn copy_table_data(
    pool: &PgPool,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let (total,): (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM {}",
        qtable(source_schema, table_name)
    ))
    .fetch_one(pool)
    .await?;

    // Disable triggers ONCE for the whole copy, hoisted out of the batch
    // loop (re-issuing the ALTER per batch re-took the ACCESS EXCLUSIVE
    // lock every time). Runs in autocommit, so it stays in effect across
    // the per-batch transactions below.
    sqlx::query(&format!(
        "ALTER TABLE {} DISABLE TRIGGER ALL",
        qtable(target_schema, table_name)
    ))
    .execute(pool)
    .await?;

    // OFFSET paging scans ever deeper into the source; keyset pagination on
    // the PK is future work.
    let mut offset = 0i64;
    let copy_result: Result<(), Box<dyn StdError + Send + Sync>> = async {
        while offset < total {
            let mut tx = pool.begin().await?;

            copy_one_batch(
                &mut tx,
                source_schema,
                target_schema,
                table_name,
                BATCH_SIZE as i64,
                offset,
            )
            .await?;

            tx.commit().await?;

            offset += BATCH_SIZE as i64;

            // Yield to other tasks and reduce DB load
            sleep(Duration::from_millis(BATCH_DELAY_MS)).await;
        }
        Ok(())
    }
    .await;

    // Re-enable triggers — also on the error path, so a failed copy does not
    // leave the target table with triggers disabled.
    sqlx::query(&format!(
        "ALTER TABLE {} ENABLE TRIGGER ALL",
        qtable(target_schema, table_name)
    ))
    .execute(pool)
    .await?;

    copy_result
}

/// Deploy path: copies all rows of one table on the caller's connection, i.e.
/// inside the caller's single transaction (no transaction management here, so
/// a failure rolls the whole overwrite back).
pub async fn copy_table_data_tx(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let (total,): (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM {}",
        qtable(source_schema, table_name)
    ))
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query(&format!(
        "ALTER TABLE {} DISABLE TRIGGER ALL",
        qtable(target_schema, table_name)
    ))
    .execute(&mut *conn)
    .await?;

    let mut offset = 0i64;
    while offset < total {
        copy_one_batch(
            conn,
            source_schema,
            target_schema,
            table_name,
            BATCH_SIZE as i64,
            offset,
        )
        .await?;
        offset += BATCH_SIZE as i64;
    }

    sqlx::query(&format!(
        "ALTER TABLE {} ENABLE TRIGGER ALL",
        qtable(target_schema, table_name)
    ))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Quotes a (possibly schema-qualified) sequence name.
fn qseq(seq: &str) -> String {
    seq.split('.').map(qident).collect::<Vec<_>>().join(".")
}

/// Gives every serial/identity column of `target_schema.table_name` a
/// target-local sequence advanced past the copied ids, so future inserts do
/// not collide. `LIKE ... INCLUDING ALL` keeps the source's sequence defaults,
/// so they have to be rewritten.
pub async fn fix_table_sequences(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    // Serial-style columns: LIKE ... INCLUDING ALL copies the default
    // verbatim, so point it at a target-local sequence instead.
    let serial_cols: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_name, column_name, substring(column_default from 'nextval\\(''([^'']+)''') \
         FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2 AND column_default LIKE 'nextval%'",
    )
    .bind(source_schema)
    .bind(table_name)
    .fetch_all(&mut *conn)
    .await?;
    // Identity columns get a fresh target-local sequence from INCLUDING
    // IDENTITY; advance it past the copied ids as well.
    let identity_cols: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name, column_name FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2 AND is_identity = 'YES'",
    )
    .bind(source_schema)
    .bind(table_name)
    .fetch_all(&mut *conn)
    .await?;
    // All columns of the table in ONE round trip via the batched builder
    // (was 4 sequential statements per serial column). sequence_fix_sql
    // quotes every catalog-derived name with qlit/qseq — never interpolate
    // sequence/table names raw into SQL literals.
    let sql = sequence_fix_sql(source_schema, target_schema, &serial_cols, &identity_cols);
    if !sql.is_empty() {
        exec_multi(conn, &sql).await?;
    }
    Ok(())
}

/// Clones the structure of every table of `source_schema` into
/// `target_schema` (creating the schema if needed) and copies the data of
/// `copy_data_tables`. Foreign keys are NOT created here — call
/// [`recreate_schema_foreign_keys`] after any seeding that must happen before
/// FK validation (e.g. role seeding).
pub async fn clone_schema_batched(
    pool: PgPool,
    source_schema: String,
    target_schema: String,
    copy_data_tables: Vec<String>,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let start_time = Instant::now();

    let mut conn = pool.acquire().await?;

    // Gather the schema metadata up front (ONE round trip), then run the whole
    // clone as ONE multi-statement batch: the per-table round trips (~300
    // statements per schema) dominated the clone time.
    let (tables, serial_cols, identity_cols) = schema_meta_conn(&mut conn, &source_schema).await?;

    let mut sql = String::new();
    // Create the target schema if it doesn't exist. CREATE SCHEMA grants
    // USAGE+CREATE to PUBLIC by default; lock it down.
    sql.push_str(&format!(
        "CREATE SCHEMA IF NOT EXISTS {};\nREVOKE ALL ON SCHEMA {} FROM PUBLIC;\n",
        qident(&target_schema),
        qident(&target_schema)
    ));
    // Clone each table structure (including constraints, indexes, etc.).
    for table_name in &tables {
        sql.push_str(&format!(
            "CREATE TABLE {} (LIKE {} INCLUDING ALL);\n",
            qtable(&target_schema, table_name),
            qtable(&source_schema, table_name),
        ));
    }
    // Copy the data of the copy-set tables. These are small system-metadata
    // tables, so one INSERT per table is fine (the old code batched at 10k
    // rows with sleeps). OVERRIDING SYSTEM VALUE is required for GENERATED
    // ALWAYS identity columns and is a no-op otherwise. Freshly LIKE-created
    // tables have no triggers, so no DISABLE TRIGGER is needed here.
    for table_name in &tables {
        if copy_data_tables.contains(table_name) {
            sql.push_str(&format!(
                "INSERT INTO {} OVERRIDING SYSTEM VALUE SELECT * FROM {};\n",
                qtable(&target_schema, table_name),
                qtable(&source_schema, table_name),
            ));
        }
    }
    sql.push_str(&sequence_fix_sql(
        &source_schema,
        &target_schema,
        &serial_cols,
        &identity_cols,
    ));
    // Run the clone batch and the app/version rewrite in ONE explicit
    // transaction: previously the rewrite ran after the batch's implicit
    // commit, so a failure there left the copied collections carrying the
    // source app_name/app_version with no meta in the target schema.
    let mut tx = conn.begin().await?;
    exec_multi(&mut tx, &sql).await?;
    rewrite_collection_app_columns(&mut tx, &target_schema).await?;
    tx.commit().await?;

    tracing::info!("Schema cloned in {:?}", start_time.elapsed());
    Ok(())
}

/// Recreates every foreign key of `target_schema` from the source schema's
/// definition (schema references remapped to the target). FKs that reference
/// an [`EXCLUDED_COPY_TABLES`] table are created `NOT VALID`: the target keeps
/// its own permission rows, so the copied rows' references cannot be validated.
pub async fn recreate_schema_foreign_keys(
    pool: &PgPool,
    source_schema: &str,
    target_schema: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let mut conn = pool.acquire().await?;
    let tables = schema_tables_conn(&mut conn, source_schema).await?;
    // ONE catalog query for the whole schema (was one SELECT per table);
    // only the ALTERs loop.
    let fks = source_schema_fks(&mut conn, source_schema, &tables).await?;
    for (table_name, fk_def, ref_table) in &fks {
        // Replace schema references in the FK definition (same-app and
        // cross-app `related_app` references alike).
        let fk_def = remap_fk_schema(fk_def, source_schema, target_schema);
        // FKs that reference an excluded (permission/log) table are created
        // NOT VALID: the target keeps its own permission rows (different
        // UUIDs), so the copied rows' references cannot be validated.
        let fk_def = if EXCLUDED_COPY_TABLES.contains(&ref_table.as_str()) {
            format!("{} NOT VALID", fk_def)
        } else {
            fk_def.clone()
        };
        sqlx::query(&format!(
            "ALTER TABLE {} ADD {}",
            qtable(target_schema, table_name),
            fk_def
        ))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Deploy direction: `source_schema` (a version) overwrites the copy-set tables
/// of `target_schema` (production), which already exists. Permissions, logs
/// and user-table rows in the target are left untouched; user tables that only
/// exist in the source are created structure-only.
///
/// The whole overwrite runs in ONE transaction on a single connection: any
/// error rolls back the TRUNCATE, the inserts, the dropped FKs and the
/// sequence fixes, so a mid-deploy failure never leaves production truncated
/// or half-written.
pub async fn overwrite_schema_batched(
    pool: PgPool,
    source_schema: String,
    target_schema: String,
    copy_data_tables: Vec<String>,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let start_time = Instant::now();

    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;
    match overwrite_schema_batched_tx(&mut tx, &source_schema, &target_schema, &copy_data_tables)
        .await
    {
        Ok(()) => {
            tx.commit().await?;
            tracing::info!("Schema overwritten in {:?}", start_time.elapsed());
            Ok(())
        }
        Err(e) => {
            // dropping the transaction would roll back too; be explicit
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

async fn overwrite_schema_batched_tx(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    copy_data_tables: &[String],
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    // Gather all metadata up front (5 round trips), then run the whole
    // overwrite as ONE multi-statement batch inside the caller's transaction:
    // the per-table round trips (~200 statements per schema) dominated the
    // overwrite time.
    let (source_tables, serial_cols, identity_cols) = schema_meta_conn(conn, source_schema).await?;
    let (target_tables, _, _) = schema_meta_conn(conn, target_schema).await?;

    // Tables that only exist in the source (e.g. a user table of a collection
    // created in the version) are cloned structure-only.
    let new_tables: Vec<&String> = source_tables
        .iter()
        .filter(|t| !target_tables.contains(t))
        .collect();

    // Record the target's FKs that touch the copy set on either side: TRUNCATE
    // is blocked by referencing tables we must not touch (permissions, logs),
    // and the copy-set tables' own FKs are recreated from the source.
    let fk_rows: Vec<(String, String, String, bool, String)> = sqlx::query_as(
        "SELECT c.conname, cl.relname, pg_get_constraintdef(c.oid), (cl.relname = ANY($2)), rl.relname \
         FROM pg_constraint c \
         JOIN pg_class cl ON cl.oid = c.conrelid \
         JOIN pg_class rl ON rl.oid = c.confrelid \
         WHERE c.contype = 'f' AND c.connamespace = $1::regnamespace \
           AND (cl.relname = ANY($2) OR rl.relname = ANY($2))",
    )
    .bind(qident(target_schema))
    .bind(&copy_data_tables.to_vec())
    .fetch_all(&mut *conn)
    .await?;

    // Source FK definitions for every table that gets FKs recreated, in one
    // query (table, constraint def, referenced table).
    let fk_tables: Vec<String> = copy_data_tables
        .iter()
        .cloned()
        .chain(new_tables.iter().map(|t| (*t).clone()))
        .collect();
    let source_fks = source_schema_fks(&mut *conn, source_schema, &fk_tables).await?;

    // FKs on user tables that already exist in the target (e.g. a relation
    // field added to an existing collection in the version): the copy set only
    // covers `alcedocore_*` metadata, and `CREATE TABLE LIKE` never copies FKs,
    // so these must be added explicitly. Queried BEFORE the batch runs, so the
    // EXISTS clause excludes tables that are only created by the batch itself
    // (those get their FKs from `source_fks` above); applied AFTER the batch,
    // because the FK's column is added by the batch's column sync.
    let missing_fks = missing_user_fks(&mut *conn, source_schema, target_schema, None).await?;

    let mut sql = String::new();
    for table_name in &new_tables {
        sql.push_str(&format!(
            "CREATE TABLE {} (LIKE {} INCLUDING ALL);\n",
            qtable(target_schema, table_name),
            qtable(source_schema, table_name),
        ));
    }
    // Sync user-table structure: columns added to user tables in the source
    // (e.g. new fields) must exist in the target too — the copy set only
    // covers the `alcedocore_*` metadata tables.
    sql.push_str(
        &missing_user_columns_sql(&mut *conn, source_schema, target_schema, None).await?,
    );
    for (conname, con_table, _def, _con_in_copy, _ref_table) in &fk_rows {
        sql.push_str(&format!(
            "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {};\n",
            qtable(target_schema, con_table),
            qident(conname),
        ));
    }
    // Empty the copy-set tables, then refill them from the source.
    if !copy_data_tables.is_empty() {
        let list = copy_data_tables
            .iter()
            .map(|t| qtable(target_schema, t))
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(&format!("TRUNCATE TABLE {};\n", list));
    }
    for table_name in copy_data_tables {
        if !target_tables.contains(table_name) && !new_tables.contains(&table_name) {
            // Copy-set table missing in the target: create it with data.
            sql.push_str(&format!(
                "CREATE TABLE {} (LIKE {} INCLUDING ALL);\n",
                qtable(target_schema, table_name),
                qtable(source_schema, table_name),
            ));
        }
        sql.push_str(&format!(
            "ALTER TABLE {} DISABLE TRIGGER ALL;\n",
            qtable(target_schema, table_name)
        ));
        sql.push_str(&format!(
            "INSERT INTO {} OVERRIDING SYSTEM VALUE SELECT * FROM {};\n",
            qtable(target_schema, table_name),
            qtable(source_schema, table_name),
        ));
        sql.push_str(&format!(
            "ALTER TABLE {} ENABLE TRIGGER ALL;\n",
            qtable(target_schema, table_name)
        ));
    }
    // rewrite_collection_app_columns, inlined (guarded on the table existing).
    if target_tables.contains(&"alcedocore_collections".to_string()) {
        if let Some((app, version)) = target_schema.split_once("010") {
            sql.push_str(&format!(
                "UPDATE {} SET app_name = {}, app_version = {};\n",
                qtable(target_schema, "alcedocore_collections"),
                qlit(app),
                qlit(version),
            ));
        }
    }
    // Recreate FKs on the copy-set tables and the new tables from the source
    // definition (schema references remapped to the target). FKs referencing
    // an excluded (permission/log) table are created NOT VALID: the target
    // keeps its own permission rows, so the copied rows' references cannot be
    // validated.
    for (table_name, fk_def, ref_table) in &source_fks {
        let fk_def = remap_fk_schema(fk_def, source_schema, target_schema);
        let fk_def = if EXCLUDED_COPY_TABLES.contains(&ref_table.as_str()) {
            format!("{} NOT VALID", fk_def)
        } else {
            fk_def
        };
        sql.push_str(&format!(
            "ALTER TABLE {} ADD {};\n",
            qtable(target_schema, table_name),
            fk_def
        ));
    }
    // Fix sequences on everything that received rows or was created.
    sql.push_str(&sequence_fix_sql(
        source_schema,
        target_schema,
        &serial_cols,
        &identity_cols,
    ));
    exec_multi(&mut *conn, &sql).await?;

    // Cross FKs (target tables outside the copy set referencing the copy set)
    // are recorded verbatim and re-added as separate statements — a cross FK
    // can be violated when the version re-created rows the target still
    // references, so each add gets the NOT VALID fallback (via a savepoint: a
    // failed statement aborts the transaction, which would otherwise make the
    // fallback impossible).
    for (_conname, con_table, def, con_in_copy, ref_table) in &fk_rows {
        if *con_in_copy {
            continue; // already recreated from the source definition
        }
        let add = format!(
            "ALTER TABLE {} ADD {}",
            qtable(target_schema, con_table),
            def,
        );
        let add = if EXCLUDED_COPY_TABLES.contains(&ref_table.as_str()) {
            format!("{} NOT VALID", add)
        } else {
            add
        };
        add_fk_not_valid_fallback(&mut *conn, &add).await?;
    }

    // Add the FKs that exist on the source's user tables but not yet on the
    // target's (relation fields added to existing collections in the version),
    // with schema references remapped to the target and a NOT VALID fallback
    // for production rows that would violate them.
    apply_missing_user_fks(&mut *conn, source_schema, target_schema, missing_fks).await?;

    Ok(())
}

pub async fn recreate_foreign_keys(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    // Fetch the source table's foreign keys via the shared batched catalog
    // query (single-table slice), with the referenced table name.
    let foreign_keys = source_schema_fks(conn, source_schema, &[table_name.to_string()]).await?;

    for (_table, fk_def, ref_table) in &foreign_keys {
        // Replace schema references in the FK definition (same-app and
        // cross-app `related_app` references alike).
        let fk_def = remap_fk_schema(fk_def, source_schema, target_schema);

        // FKs that reference an excluded (permission/log) table are created
        // NOT VALID: the target keeps its own permission rows (different
        // UUIDs), so the copied rows' references cannot be validated.
        let fk_def = if EXCLUDED_COPY_TABLES.contains(&ref_table.as_str()) {
            format!("{} NOT VALID", fk_def)
        } else {
            fk_def
        };

        // Execute the FK creation
        sqlx::query(&format!(
            "ALTER TABLE {} ADD {}",
            qtable(target_schema, table_name),
            fk_def
        ))
        .execute(&mut *conn)
        .await?;
    }

    Ok(())
}

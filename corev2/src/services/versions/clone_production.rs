use sqlx::PgPool;
use std::error::Error as StdError;
use std::time::Instant;
use tokio::time::{Duration, sleep};

const BATCH_SIZE: usize = 10_000; // rows per batch
const BATCH_DELAY_MS: u64 = 50; // pause between batches to ease DB pressure

pub async fn clone_schema_batched(
    pool: PgPool,
    source_schema: String,
    target_schema: String,
    copy_data_tables: Vec<String>,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    let start_time = Instant::now();

    // Create the target schema if it doesn't exist
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {}", target_schema))
        .execute(&pool)
        .await?;

    // Fetch all tables in the source schema
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = $1 AND table_type = 'BASE TABLE'",
    )
    .bind(&source_schema)
    .fetch_all(&pool)
    .await?;

    // Clone each table structure (including constraints, indexes, etc.)
    for (table_name,) in &tables {
        println!("Cloning table: {}.{}", source_schema, table_name);

        sqlx::query(&format!(
            "CREATE TABLE {target}.{table} (LIKE {source}.{table} INCLUDING ALL)",
            target = target_schema,
            source = source_schema,
            table = table_name,
        ))
        .execute(&pool)
        .await?;

        // Copy data in batches if the table is in the copy_data_tables list
        if copy_data_tables.contains(table_name) {
            // Get total row count for batching
            let (total,): (i64,) = sqlx::query_as(&format!(
                "SELECT COUNT(*) FROM {}.{}",
                source_schema, table_name
            ))
            .fetch_one(&pool)
            .await?;

            println!("  {} rows to copy", total);

            let mut offset = 0i64;
            while offset < total {
                let mut tx = pool.begin().await?;

                // Disable triggers to speed up bulk inserts
                sqlx::query(&format!(
                    "ALTER TABLE {}.{} DISABLE TRIGGER ALL",
                    target_schema, table_name
                ))
                .execute(&mut *tx)
                .await?;

                // Copy a batch of rows
                sqlx::query(&format!(
                    "INSERT INTO {target}.{table}
                     SELECT * FROM {source}.{table}
                     ORDER BY 1  -- stable order for paging
                     LIMIT {limit} OFFSET {offset}",
                    target = target_schema,
                    source = source_schema,
                    table = table_name,
                    limit = BATCH_SIZE,
                    offset = offset,
                ))
                .execute(&mut *tx)
                .await?;

                // Re-enable triggers
                sqlx::query(&format!(
                    "ALTER TABLE {}.{} ENABLE TRIGGER ALL",
                    target_schema, table_name
                ))
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;

                offset += BATCH_SIZE as i64;
                println!("  {}/{} rows copied", offset.min(total), total);

                // Yield to other tasks and reduce DB load
                sleep(Duration::from_millis(BATCH_DELAY_MS)).await;
            }
        }
    }

    // Recreate FKs after all tables are cloned
    for (table_name,) in &tables {
        recreate_foreign_keys(&pool, &source_schema, &target_schema, table_name).await?;
    }

    println!("Schema cloned in {:?}", start_time.elapsed());
    Ok(())
}

pub async fn recreate_foreign_keys(
    pool: &PgPool,
    source_schema: &str,
    target_schema: &str,
    table_name: &str,
) -> Result<(), Box<dyn StdError + Send + Sync>> {
    // Fetch foreign keys from the source table
    let foreign_keys: Vec<(String,)> = sqlx::query_as(
        "SELECT pg_get_constraintdef(oid)
         FROM pg_constraint
         WHERE conrelid = $1::regclass AND contype = 'f'",
    )
    .bind(format!("{}.{}", source_schema, table_name))
    .fetch_all(pool)
    .await?;

    for (fk_def,) in foreign_keys {
        // Replace schema references in the FK definition
        let fk_def = if source_schema.starts_with("alcedocore_") {
            fk_def
        } else {
            fk_def.replace(
                &format!("{}.{}", source_schema, table_name),
                &format!("{}.{}", target_schema, table_name),
            )
        };

        // Execute the FK creation
        sqlx::query(&format!(
            "ALTER TABLE {}.{} ADD {}",
            target_schema, table_name, fk_def
        ))
        .execute(pool)
        .await?;
    }

    Ok(())
}

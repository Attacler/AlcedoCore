#[macro_export]
macro_rules! find_all {
    ($fn_name:ident, $table:expr, $cols:expr, $order:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool) -> Result<Vec<Self>, crate::error::AppError> {
            let rows = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " ORDER BY ", $order)
            )
            .fetch_all(db)
            .await?;
            Ok(rows)
        }
    };
}

#[macro_export]
macro_rules! find_by {
    ($fn_name:ident, $table:expr, $cols:expr, $key_col:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool, key: &str) -> Result<Option<Self>, crate::error::AppError> {
            let row = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " WHERE ", $key_col, " = $1")
            )
            .bind(key)
            .fetch_optional(db)
            .await?;
            Ok(row)
        }
    };
}

#[macro_export]
macro_rules! delete_by {
    ($fn_name:ident, $table:expr, $key_col:expr, $key_ty:ty) => {
        pub async fn $fn_name(db: &sqlx::PgPool, key: $key_ty) -> Result<(), crate::error::AppError> {
            sqlx::query(concat!("DELETE FROM ", $table, " WHERE ", $key_col, " = $1"))
                .bind(key)
                .execute(db)
                .await?;
            Ok(())
        }
    };
}

#[macro_export]
macro_rules! find_by_where {
    ($fn_name:ident, $table:expr, $cols:expr, $key_col:expr, $extra_where:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool, key: &str) -> Result<Option<Self>, crate::error::AppError> {
            let row = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " WHERE ", $key_col, " = $1 AND ", $extra_where)
            )
            .bind(key)
            .fetch_optional(db)
            .await?;
            Ok(row)
        }
    };
}

#[macro_export]
macro_rules! find_all_where {
    ($fn_name:ident, $table:expr, $cols:expr, $where:expr, $order:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool) -> Result<Vec<Self>, crate::error::AppError> {
            let rows = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " WHERE ", $where, " ORDER BY ", $order)
            )
            .fetch_all(db)
            .await?;
            Ok(rows)
        }
    };
}

#[macro_export]
macro_rules! find_all_where_bind {
    ($fn_name:ident, $table:expr, $cols:expr, $where:expr, $order:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool, key: &str) -> Result<Vec<Self>, crate::error::AppError> {
            let rows = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " WHERE ", $where, " ORDER BY ", $order)
            )
            .bind(key)
            .fetch_all(db)
            .await?;
            Ok(rows)
        }
    };
}

#[macro_export]
macro_rules! find_by_two {
    ($fn_name:ident, $table:expr, $cols:expr, $col1:expr, $col2:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool, key1: &str, key2: &str) -> Result<Option<Self>, crate::error::AppError> {
            let row = sqlx::query_as::<_, Self>(
                concat!("SELECT ", $cols, " FROM ", $table, " WHERE ", $col1, " = $1 AND ", $col2, " = $2")
            )
            .bind(key1)
            .bind(key2)
            .fetch_optional(db)
            .await?;
            Ok(row)
        }
    };
}

#[macro_export]
macro_rules! update_by_slug_version {
    ($fn_name:ident, $set_col:expr) => {
        pub async fn $fn_name(db: &sqlx::PgPool, slug: &str, version: &str, value: &str) -> Result<(), crate::error::AppError> {
            sqlx::query(
                concat!("UPDATE plugin_versions SET ", $set_col, " = $3 WHERE slug = $1 AND version = $2")
            )
            .bind(slug)
            .bind(version)
            .bind(value)
            .execute(db)
            .await?;
            Ok(())
        }
    };
}

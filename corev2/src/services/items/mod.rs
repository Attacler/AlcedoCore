pub mod query;
pub mod service;

/// Builds an item payload `Map` for `ItemsService` from `"key" => value` pairs.
/// Values are converted with `serde_json::json!`.
///
/// ```ignore
/// let payload = item_map! {
///     "name" => name,
///     "is_active" => true,
/// };
/// ```
#[macro_export]
macro_rules! item_map {
    ( $( $key:expr => $value:expr ),* $(,)? ) => {{
        let mut map = ::serde_json::Map::new();
        $(
            map.insert($key.to_string(), ::serde_json::json!($value));
        )*
        map
    }};
}
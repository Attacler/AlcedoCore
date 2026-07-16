//! Shared utilities for binding `serde_json::Value` to sqlx query builders.
//!
//! Two macros are provided:
//!
//! * [`bind_json_value!`] — for **borrowed** values (`&serde_json::Value`). Binds strings
//!   via `as_str()` (borrowing) and bools via `*b` (dereferencing `&bool`).
//!
//! * [`bind_json_value_owned!`] — for **owned** values (`serde_json::Value`). Binds strings
//!   via move (`$q.bind(s)`) and bools directly (`$q.bind(b)`), avoiding lifetime issues
//!   when the owned value is short-lived (e.g. created inside a loop).
//!
//! Both work with `sqlx::query`, `sqlx::query_as`, and `sqlx::query_scalar` since
//! `.bind()` returns the same builder type for each call.
//!
//! # Usage
//!
//! ```ignore
//! use crate::bind_json_value;
//!
//! // Borrowed values (most common — iterate over &Vec<Value>)
//! let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
//! for val in &bind_values {
//!     q = bind_json_value!(q, val);
//! }
//!
//! // Owned values (when you must clone / create a Value locally)
//! let val = some_owned_value();
//! q = bind_json_value_owned!(q, val); // val is moved
//! ```

/// Bind a **borrowed** `&serde_json::Value` to a sqlx query builder.
///
/// Use this when iterating over `&Vec<Value>` or `&[Value]`. Strings are
/// borrowed via `.as_str()` and bools dereferenced via `*b`.
#[macro_export]
macro_rules! bind_json_value {
    ($q:expr, $val:expr) => {
        match $val {
            serde_json::Value::Null => {
                let v: Option<String> = None;
                $q.bind(v)
            }
            serde_json::Value::String(s) => $q.bind(s.as_str()),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    $q.bind(i)
                } else if let Some(f) = n.as_f64() {
                    $q.bind(f)
                } else {
                    $q.bind(n.to_string())
                }
            }
            serde_json::Value::Bool(b) => $q.bind(*b),
            serde_json::Value::Array(arr) => {
                $q.bind(serde_json::to_string(arr).unwrap_or_default())
            }
            serde_json::Value::Object(obj) => {
                $q.bind(serde_json::to_string(obj).unwrap_or_default())
            }
        }
    };
}

/// Bind an **owned** `serde_json::Value` to a sqlx query builder.
///
/// Use this when the value is created locally (e.g. cloned from a map
/// inside a loop) and would not live long enough for `bind_json_value!`
/// to borrow from it. Owned strings are moved into the query and bools
/// are copied (bool is `Copy`).
#[macro_export]
macro_rules! bind_json_value_owned {
    ($q:expr, $val:expr) => {
        match $val {
            serde_json::Value::Null => {
                let v: Option<String> = None;
                $q.bind(v)
            }
            serde_json::Value::String(s) => $q.bind(s),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    $q.bind(i)
                } else if let Some(f) = n.as_f64() {
                    $q.bind(f)
                } else {
                    $q.bind(n.to_string())
                }
            }
            serde_json::Value::Bool(b) => $q.bind(b),
            serde_json::Value::Array(arr) => {
                $q.bind(serde_json::to_string(&arr).unwrap_or_default())
            }
            serde_json::Value::Object(obj) => {
                $q.bind(serde_json::to_string(&obj).unwrap_or_default())
            }
        }
    };
}

#[cfg(test)]
mod tests {
    //! Compile-and-behavior tests for the `bind_json_value!` macro.
    //!
    //! We use a simple builder mock that mimics sqlx's `bind` → `Self` pattern
    //! to verify the macro expands correctly and each JSON variant is handled.

    /// Mock binder that records values passed to `bind()` as debug strings.
    struct MockBinder(Vec<String>);

    impl MockBinder {
        fn bind<T: std::fmt::Debug>(mut self, val: T) -> Self {
            self.0.push(format!("{:?}", val));
            self
        }
    }

    #[test]
    fn test_bind_null() {
        let q = MockBinder(vec![]);
        let val = serde_json::Value::Null;
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1, "should bind exactly one value");
    }

    #[test]
    fn test_bind_string() {
        let q = MockBinder(vec![]);
        let val = serde_json::Value::String("hello".to_string());
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_number_int() {
        let q = MockBinder(vec![]);
        let val = serde_json::Value::Number(serde_json::Number::from(42));
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_number_float() {
        let q = MockBinder(vec![]);
        let val = serde_json::json!(3.14);
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_bool() {
        let q = MockBinder(vec![]);
        let val = serde_json::Value::Bool(true);
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_array() {
        let q = MockBinder(vec![]);
        let val = serde_json::json!([1, 2, 3]);
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_object() {
        let q = MockBinder(vec![]);
        let val = serde_json::json!({"key": "value"});
        let q = bind_json_value!(q, &val);
        assert_eq!(q.0.len(), 1);
    }

    #[test]
    fn test_bind_via_reference_iterator() {
        // Simulate the most common usage: iterating over &Vec<Value>
        let values = vec![
            serde_json::Value::Null,
            serde_json::Value::String("foo".into()),
            serde_json::Value::Bool(false),
            serde_json::json!(42),
        ];
        let mut q = MockBinder(vec![]);
        for val in &values {
            q = bind_json_value!(q, val);
        }
        assert_eq!(q.0.len(), 4);
    }
}

/// Generate a `#[async_trait]` impl block delegating to `self.<field>`.
#[macro_export]
macro_rules! delegate_self_impl {
    ($trait:path, $ty:ty, $field:ident, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),*) -> $ret:ty;)*
    }) => {
        #[::async_trait::async_trait]
        impl $trait for $ty {
            $(
                async fn $name(&self, $($arg: $arg_ty),*) -> $ret {
                    self.$field.$name($($arg),*).await
                }
            )*
        }
    };
}

/// Generate a `#[async_trait]` impl block delegating to a static target.
#[macro_export]
macro_rules! delegate_impl {
    ($trait:path, $ty:ty, $target:path, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),*) -> $ret:ty;)*
    }) => {
        #[::async_trait::async_trait]
        impl $trait for $ty {
            $(
                async fn $name(&self, $($arg: $arg_ty),*) -> $ret {
                    $target.$name($($arg),*).await
                }
            )*
        }
    };
}

/// Generate individual methods delegating to `self.<field>`, within an existing
/// `#[async_trait]` impl block.  Uses explicit `Pin<Box<dyn Future>>` matching
/// what `#[async_trait]` produces.  Reference args MUST use `&'async_trait`.
#[macro_export]
macro_rules! delegate_pinned {
    ($field:ident, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret:ty;)*
    }) => {
        $(
            fn $name<'async_trait>(
                &'async_trait self,
                $($arg: $arg_ty),*
            ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = $ret> + ::std::marker::Send + 'async_trait>>
            {
                Box::pin(async move { self.$field.$name($($arg),*).await })
            }
        )*
    };
}

/// Like `delegate_pinned!` but delegates to a static target.
#[macro_export]
macro_rules! delegate_pinned_to {
    ($target:path, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret:ty;)*
    }) => {
        $(
            fn $name<'async_trait>(
                &'async_trait self,
                $($arg: $arg_ty),*
            ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = $ret> + ::std::marker::Send + 'async_trait>>
            {
                Box::pin(async move { $target.$name($($arg),*).await })
            }
        )*
    };
}

/// Generate a full `impl` block (without `#[async_trait]`) delegating to
/// `self.<field>`.  Uses explicit `Pin<Box<dyn Future>>` signatures matching
/// what `#[async_trait]` produces on the trait side so this block can coexist
/// with a `#[async_trait]` impl block for the same trait.
/// Reference args MUST use `&'async_trait` lifetime syntax.
#[macro_export]
macro_rules! delegate_self_explicit {
    ($trait:path, $ty:ty, $field:ident, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret:ty;)*
    }) => {
        impl $trait for $ty {
            $(
                fn $name<'async_trait>(
                    &'async_trait self,
                    $($arg: $arg_ty),*
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = $ret> + ::std::marker::Send + 'async_trait>>
                {
                    Box::pin(async move { self.$field.$name($($arg),*).await })
                }
            )*
        }
    };
}

/// Like `delegate_self_explicit!` but delegates to a static target.
#[macro_export]
macro_rules! delegate_explicit {
    ($trait:path, $ty:ty, $target:path, {
        $(fn $name:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret:ty;)*
    }) => {
        impl $trait for $ty {
            $(
                fn $name<'async_trait>(
                    &'async_trait self,
                    $($arg: $arg_ty),*
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = $ret> + ::std::marker::Send + 'async_trait>>
                {
                    Box::pin(async move { $target.$name($($arg),*).await })
                }
            )*
        }
    };
}

#[macro_export]
macro_rules! tags {
    ($($key:ident: $value:expr),* $(,)?) => {
        {
            let mut map = ::std::collections::HashMap::new();
            $(
                map.insert(stringify!($key).to_string(), $value.to_string());
            )*
            Some(map)
        }
    };
}

/// A simple macro to serialize a value to bytes.
macro_rules! serialize {
    ($expr:expr) => {
        ::rkyv::api::high::to_bytes_in::<_, ::rkyv::rancor::Error>($expr, Vec::new())
    };
}

/// A simple macro to access a value from bytes.
#[allow(unused_macros)]
macro_rules! access {
    ($ty:ty, $expr:expr) => {
        ::rkyv::access::<$ty, ::rkyv::rancor::Error>($expr)
    };
}

/// A simple macro to deserialize a value from bytes.
macro_rules! deserialize_bytes {
    ($ty:ty, $expr:expr) => {{
        let ro = ::rkyv::access::<$ty, ::rkyv::rancor::Error>($expr);
        match ro {
            Ok(v) => ::rkyv::deserialize::<_, ::rkyv::rancor::Error>(v),
            Err(e) => Err(e),
        }
    }};
}

/// A simple macro to deserialize a value from its archived form.
#[allow(unused_macros)]
macro_rules! deserialize_archived {
    ($expr:expr) => {
        ::rkyv::deserialize::<_, ::rkyv::rancor::Error>($expr)
    };
}

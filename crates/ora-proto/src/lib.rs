#![allow(missing_docs)]

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod common {
    #[path = "generated/ora.common.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod server {
    #[path = "generated/ora.server.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod snapshot {
    #[path = "generated/ora.snapshot.v1.rs"]
    pub mod v1;
}

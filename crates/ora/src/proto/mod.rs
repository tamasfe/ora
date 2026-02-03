//! Genetated protobuf client and types, re-exported for convenience.
#![allow(missing_docs)]

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod admin {
    #[path = "generated/ora.admin.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod common {
    #[path = "generated/ora.common.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod executors {
    #[path = "generated/ora.executors.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod jobs {
    #[path = "generated/ora.jobs.v1.rs"]
    pub mod v1;
}

#[allow(clippy::pedantic, clippy::all)]
#[path = ""]
pub mod schedules {
    #[path = "generated/ora.schedules.v1.rs"]
    pub mod v1;
}

pub(crate) mod convert;
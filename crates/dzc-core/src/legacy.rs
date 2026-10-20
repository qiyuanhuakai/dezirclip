//! Unstable migration bridge for the existing desktop adapter.
//! Consumers should use api, ports and capture. This module is not a compatibility promise.
pub mod database {
    pub use crate::database::*;
}
pub mod infrastructure {
    pub use crate::infrastructure::*;
}
pub mod services {
    pub use crate::services::*;
}

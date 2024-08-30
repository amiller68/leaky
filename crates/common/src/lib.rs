mod api;
#[allow(unused_imports)]
#[allow(dead_code)]
mod ipfs_rpc;
mod mount;
mod types;

pub mod prelude {
    pub use crate::api::api_requests;
    pub use crate::mount::{BlockCache, Mount, MountError};
    pub use crate::types::{Cid, Ipld, Manifest, Object, Version};
}

pub mod error {
    pub use crate::mount::MountError;
    pub use crate::types::CidError;
}

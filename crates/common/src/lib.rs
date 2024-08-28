mod block_store;
#[allow(unused_imports)]
#[allow(dead_code)]
mod ipfs_rpc;
mod leaky;
mod leaky_api;
mod types;

pub mod prelude {
    pub use crate::block_store::BlockStore;
    pub use crate::leaky::{BlockCache, Leaky, LeakyError};
    pub use crate::types::{Cid, Ipld, Manifest, Object, Version};
}

pub mod error {
    pub use crate::leaky::LeakyError;
    pub use crate::types::CidError;
}

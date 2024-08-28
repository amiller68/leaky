use std::error::Error;

use async_trait::async_trait;

use crate::types::{Block, Cid, IpldCodec, MhCode};

const DEFAULT_MH_CODE: MhCode = MhCode::Blake3_256;
const DEFAULT_IPLD_CODEC: IpldCodec = IpldCodec::DagCbor;

#[async_trait]
pub trait BlockStore {
    type BlockStoreError<'a>: Error + 'a;

    async fn put_opts<B>(
        &self,
        block: &B,
        codec: IpldCodec,
        mh_code: MhCode,
    ) -> Result<Cid, BlockStoreError>
    where
        B: Into<Ipld> + Clone + Send + Sync + 'static;

    async fn get<B>(&self, cid: &Cid) -> Result<Option<B>, BlockStoreError>
    where
        B: From<Ipld> + Clone + Send + Sync + 'static;

    async fn put<B>(&self, block: &B) -> Result<Cid, BlockStoreError>
    where
        B: Into<Ipld> + Clone + Send + Sync + 'static,
    {
        self.put_opts(block, DEFAULT_IPLD_CODEC, DEFAULT_MH_CODE)
            .await
    }
}

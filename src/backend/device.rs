use std::io::Cursor;
use std::io::Read;
use std::path::PathBuf;

use cid::Cid;
use ethers::signers::LocalWallet;
use ethers::types::Address;
use futures_util::stream::TryStreamExt;

use crate::eth::{EthClient, EthClientError, RootCid};
use crate::ipfs::{
    add_data_request, hash_data_request, IpfsApi, IpfsClient, IpfsClientError, IpfsError,
    IpfsGateway,
};

use crate::types::Manifest;

/// Union of IPFS and Ethereum clients for coordinating pushing and pulling
/// dor-store updates to and from remote infrastructure.
/// It is NOT a reflection of dor-store state. This state should be handled
/// by your application.
pub struct Device {
    /// Address for the contract hosting our RootCid
    contract_address: Address,
    /// IpfsClient for communicating with local staging
    local_ipfs_client: IpfsClient,
    /// IpfsClient for communicating with remote pinning service
    ipfs_client: IpfsClient,
    /// IpfsGateway for pulling data from a public gateway
    ipfs_gateway: IpfsGateway,
    /// EthClient for reading and updating a root cid. The contract address should be
    /// callable from this client
    eth: EthClient,
    /// LocalWallet for signing RootCid updates
    wallet: LocalWallet,
}

/// One stop shop for coordinating interactions with a given remote configuration
impl Device {
    pub fn new(
        contract_address: Address,
        local_ipfs_client: IpfsClient,
        ipfs_client: IpfsClient,
        ipfs_gateway: IpfsGateway,
        eth: EthClient,
        wallet: LocalWallet,
    ) -> Self {
        Self {
            contract_address,
            eth,
            local_ipfs_client,
            ipfs_client,
            ipfs_gateway,
            wallet,
        }
    }

    /// Set the LocalWallet for the device
    pub fn with_wallet(mut self, wallet: LocalWallet) -> Self {
        self.wallet = wallet;
        self
    }

    /* Dor Store Helpers */

    /// Read a Block by its Cid as a Manifest from Ipfs
    /// # Args
    /// - cid: The cid of the Manifest object
    /// - remote: whether to read against the remote of local IPFS client
    pub async fn read_manifest(&self, cid: &Cid, remote: bool) -> Result<Manifest, DeviceError> {
        let manifest_data = self.read_ipfs_data(cid, remote).await?;
        let manifest = serde_json::from_slice(&manifest_data)?;
        Ok(manifest)
    }

    /// Write a Manifest as a block on Ipfs
    /// # Args
    /// - remote: whether to write against the remote of local IPFS client
    /// # Returns the Cid of the Manifest object
    pub async fn write_manifest(
        &self,
        manifest: &Manifest,
        remote: bool,
    ) -> Result<Cid, DeviceError> {
        let manifest_data = serde_json::to_vec(&manifest)?;
        let manifest_data = Cursor::new(manifest_data);
        let cid = self.write_ipfs_data(manifest_data, remote).await?;
        Ok(cid)
    }

    /// Hash a Manifest object against Ipfs
    /// # Args
    /// - manifest: the Manifest instance to hash
    /// - remote: whether to hash against the remote or local IPFS client
    /// # Returns the Cid of the Manifest object
    pub async fn hash_manifest(
        &self,
        manifest: &Manifest,
        remote: bool,
    ) -> Result<Cid, DeviceError> {
        let manifest_data = serde_json::to_vec(&manifest)?;
        let manifest_data = Cursor::new(manifest_data);
        let cid = self.hash_ipfs_data(manifest_data, remote).await?;
        Ok(cid)
    }

    /* Eth Helpers */

    /// Get the chain id in use
    pub fn chain_id(&self) -> u32 {
        self.eth.chain_id()
    }

    /// Read the root cid from the eth remote
    pub async fn read_root_cid(&self) -> Result<Cid, DeviceError> {
        let root_cid = RootCid::new(self.eth.clone(), self.contract_address, None)?;
        let root_cid = root_cid.read().await?;
        Ok(root_cid)
    }

    /// Update the root cid against the eth remote
    /// # Args
    /// - previous_root_cid: the previously known root cid of the remote
    /// - next_root_cid: the root cid to overwrite it with
    pub async fn update_root_cid(
        &self,
        previous_root_cid: Cid,
        next_root_cid: Cid,
    ) -> Result<(), DeviceError> {
        let root_cid = RootCid::new(
            self.eth.clone(),
            self.contract_address,
            Some(self.wallet.clone()),
        )?;

        let _maybe_txn_reciept = root_cid.update(previous_root_cid, next_root_cid).await?;

        // TODO: maybe should wait for emitted event and check for a valid update

        Ok(())
    }

    /* Ipfs Helpers */

    /// Get the PeerId for the configured for either of our IpfsClients
    /// # Args
    /// - remote: whether to get the PeerId of the remote or local instance
    pub async fn ipfs_id(&self, remote: bool) -> Result<String, DeviceError> {
        let id_response = if remote {
            self.ipfs_client.id(None)
        } else {
            self.local_ipfs_client.id(None)
        }
        .await?;
        let id = id_response.id;
        Ok(id)
    }

    // TODO: Check for links, keep pulling if any
    // TODO: Add method for just returning the stream
    /// Read a block by its cid against the configured IpfsClients
    /// # Args
    /// - cid: the cid to read
    /// - remote: whether to do so against a remote or local instance
    pub async fn read_ipfs_data(&self, cid: &Cid, remote: bool) -> Result<Vec<u8>, DeviceError> {
        let block_stream = if remote {
            self.ipfs_client.block_get(&cid.to_string())
        } else {
            self.local_ipfs_client.block_get(&cid.to_string())
        };
        let block_data = block_stream
            .map_ok(|chunk| chunk.to_vec())
            .try_concat()
            .await?;
        Ok(block_data)
    }

    /// Read a Cid from the configured Ipfs Gateway
    /// # Args
    /// - cid: the cid to read
    /// - path: Optional path parameter if the Cid points to a unix-fs directory
    pub async fn read_ipfs_gateway_data(
        &self,
        cid: &Cid,
        path: Option<PathBuf>,
    ) -> Result<Vec<u8>, DeviceError> {
        let data = self.ipfs_gateway.get(cid, path).await?;
        Ok(data)
    }

    /// Write data against the configured IpfsClients
    /// # Args
    /// - data: the data to write
    /// - remote: whether to do so against a remote or local instance
    /// # Returns the cid of the wrote data
    pub async fn write_ipfs_data<R>(&self, data: R, remote: bool) -> Result<Cid, DeviceError>
    where
        R: 'static + Read + Send + Sync + Unpin,
    {
        let add_response = if remote {
            self.ipfs_client.add_with_options(data, add_data_request())
        } else {
            self.local_ipfs_client
                .add_with_options(data, add_data_request())
        }
        .await?;
        let hash = add_response.hash;
        let cid = Cid::try_from(hash)?;
        Ok(cid)
    }

    /// Hash data against the configured IpfsClients
    /// # Args
    /// - data: the data to write
    /// - remote: whether to do so against a remote or local instance
    /// # Returns the cid of the wrote data
    pub async fn hash_ipfs_data<R>(&self, data: R, remote: bool) -> Result<Cid, DeviceError>
    where
        R: 'static + Read + Send + Sync + Unpin,
    {
        let add_response = if remote {
            self.ipfs_client.add_with_options(data, hash_data_request())
        } else {
            self.local_ipfs_client
                .add_with_options(data, hash_data_request())
        }
        .await?;
        let hash = add_response.hash;
        let cid = Cid::try_from(hash)?;
        Ok(cid)
    }

    /// Stat the presence of a block against the configured IpfsClients
    /// # Args
    /// - cid: the cid to check
    /// - remote: whether to do so against a remote or local instance
    /// # Returns the size of the queried block
    pub async fn _stat_ipfs_data(
        &self,
        cid: &Cid,
        remote: bool,
    ) -> Result<Option<u64>, DeviceError> {
        let cid = cid.to_string();
        let stat_response = if remote {
            self.ipfs_client.block_stat(&cid)
        } else {
            self.local_ipfs_client.block_stat(&cid)
        }
        .await;
        match stat_response {
            Ok(stat) => Ok(Some(stat.size)),
            Err(IpfsClientError::Api(api_error)) => {
                if api_error.code == 0 && api_error.message == "blockservice: key not found" {
                    Ok(None)
                } else {
                    Err(DeviceError::IpfsClient(IpfsClientError::Api(api_error)))
                }
            }
            Err(e) => Err(DeviceError::IpfsClient(e)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("cid error: {0}")]
    Cid(#[from] cid::Error),
    #[error("ipfs error: {0}")]
    Ipfs(#[from] IpfsError),
    #[error("ipfs error: {0}")]
    IpfsClient(#[from] IpfsClientError),
    #[error("eth error: {0}")]
    EthClient(#[from] EthClientError),
    #[error("root cid error: {0}")]
    RootCid(#[from] crate::eth::RootCidError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

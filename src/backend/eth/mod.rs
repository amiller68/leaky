use std::fmt::Display;
use std::sync::Arc;

use ethers::{
    abi::Abi,
    contract::Contract,
    prelude::*,
    providers::{Http, Provider},
    signers::LocalWallet,
    types::Address,
};
use serde::{Deserialize, Serialize};
use url::Url;

mod cid_token;
mod root_cid;

pub use root_cid::{RootCid, RootCidError};

/// Connection to an HTTP RPC API for an EVM-based chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthRemote {
    pub rpc_url: Url,
    pub chain_id: u32,
}

impl Display for EthRemote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rpc: {}, chain_id: {}", self.rpc_url, self.chain_id)
    }
}

/// Client for interacting with the EVM over Http
#[derive(Debug, Clone)]
pub struct EthClient {
    provider: Provider<Http>,
    chain_id: u32,
    contract: Option<Contract<ethers::providers::Provider<Http>>>,
    signer: Option<SignerMiddleware<Provider<Http>, LocalWallet>>,
}

impl TryFrom<EthRemote> for EthClient {
    type Error = EthClientError;

    fn try_from(remote: EthRemote) -> Result<Self, Self::Error> {
        let provider = Provider::<Http>::try_from(remote.rpc_url.to_string())
            .map_err(|e| EthClientError::Default(e.to_string()))?;
        Ok(Self {
            provider,
            chain_id: remote.chain_id,
            contract: None,
            signer: None,
        })
    }
}

impl EthClient {
    /// Get the configured chain id
    pub fn chain_id(&self) -> u32 {
        self.chain_id
    }

    /// Get the contract from the implementing struct
    pub fn contract(&self) -> Option<Contract<Provider<Http>>> {
        self.contract.clone()
    }

    // #[cfg(not(target_arch = "wasm32"))]
    // /// Get the provider from the implementing struct
    // pub fn provider(&self) -> Provider<Http> {
    //     self.provider.clone()
    // }

    #[cfg(not(target_arch = "wasm32"))]
    /// Get the signer from the implementing struct
    pub fn signer(&self) -> Option<SignerMiddleware<Provider<Http>, LocalWallet>> {
        self.signer.clone()
    }

    /// Attach SignerMiddleware to the client
    pub fn with_signer(mut self, wallet: LocalWallet) -> Self {
        let wallet = wallet.with_chain_id(self.chain_id);
        let signer = SignerMiddleware::new(self.provider.clone(), wallet);
        self.signer = Some(signer);
        self
    }

    /// Attach a Contract to the client
    pub fn with_contract(mut self, address: Address, abi: Abi) -> Self {
        let contract = Contract::new(address, abi, Arc::new(self.provider.clone()));
        self.contract = Some(contract);
        self
    }
}

// TODO: oof error types
#[derive(thiserror::Error, Debug)]
pub enum EthClientError {
    #[error("default error: {0}")]
    Default(String),
}

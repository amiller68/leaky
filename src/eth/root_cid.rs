use cid::Cid;
use ethers::{abi::Abi, signers::LocalWallet, types::Address};
use serde_json::Value;

#[cfg(not(target_arch = "wasm32"))]
use ethers::{
    prelude::*,
    types::{TransactionReceipt, TransactionRequest},
};

use super::cid_token::CidToken;
use super::{EthClient, EthClientError};

const ABI_STRING: &str = include_str!("../../out/RootCid.sol/RootCid.json");

/// Wrapper around an EthClient for interacting with our RootCid contract
pub struct RootCid(EthClient);

impl RootCid {
    pub fn new(
        eth_client: EthClient,
        address: Address,
        signer: Option<LocalWallet>,
    ) -> Result<Self, RootCidError> {
        let eth_client = match signer {
            Some(signer) => eth_client.with_signer(signer),
            None => eth_client,
        };
        let abi_value: Value = serde_json::from_str(ABI_STRING)?;
        let abi: Abi = serde_json::from_value(abi_value["abi"].clone())?;

        let client = eth_client.with_contract(address, abi);
        let client = client.clone();
        Ok(Self(client.clone()))
    }

    // TODO: grant writer workflow -- for now everything is admin controlled
    // /// Grant the given address the ability to update the contract cid
    // pub async fn grant_writer(
    //     &self,
    //     _grantee_address: Address,
    // ) -> Result<Option<TransactionReceipt>, RootCidError> {
    //     // TODO: This is janky, but we should have the contract available by now
    //     let contract = self.0.contract().unwrap();
    //     let address = contract.address();
    //     let chain_id = self.0.chain_id();
    //     let signer = match self.0.signer() {
    //         Some(signer) => signer,
    //         None => return Err(RootCidError::MissingSigner),
    //     };

    //     let data = contract
    //         .encode("grantWriter", (address,))
    //         .map_err(|e| RootCidError::Default(e.to_string()))?;

    //     let tx = TransactionRequest::new()
    //         .to(contract.address())
    //         .data(data)
    //         .chain_id(chain_id);
    //     let signed_tx = signer
    //         .send_transaction(tx, None)
    //         .await
    //         .map_err(|e| RootCidError::Default(e.to_string()))?;
    //     let reciept = signed_tx
    //         .await
    //         .map_err(|e| RootCidError::Default(e.to_string()))?;
    //     Ok(reciept)
    // }

    /* CRUD */

    /// Read the current cid from the contract
    pub async fn read(&self) -> Result<Cid, RootCidError> {
        // TODO: This is janky, but we should have the contract available by now
        let contract = self.0.contract().unwrap();

        let cid: Cid = contract
            .method::<_, CidToken>("read", ())
            .map_err(|e| RootCidError::Default(e.to_string()))?
            .call()
            .await
            .map_err(|e| RootCidError::Default(e.to_string()))?
            .into();
        Ok(cid)
    }

    // Note: the web client never writes to the contract
    #[cfg(not(target_arch = "wasm32"))]
    /// Update the current cid in the contract
    /// Requires a signer
    pub async fn update(
        &self,
        previous_cid: Cid,
        cid: Cid,
    ) -> Result<Option<TransactionReceipt>, RootCidError> {
        // TODO: This is janky, but we should have the contract available by now
        let contract = self.0.contract().unwrap();
        let chain_id = self.0.chain_id();
        let signer = match self.0.signer() {
            Some(signer) => signer,
            None => return Err(RootCidError::MissingSigner),
        };
        let data = contract
            .encode(
                "update",
                (CidToken::from(previous_cid), CidToken::from(cid)),
            )
            .map_err(|e| RootCidError::Default(e.to_string()))?;
        let tx = TransactionRequest::new()
            .to(contract.address())
            .data(data)
            .chain_id(chain_id);
        let signed_tx = signer
            .send_transaction(tx, None)
            .await
            .map_err(|e| RootCidError::Default(e.to_string()))?;
        println!("Signed tx: {:?}", signed_tx);
        let reciept = signed_tx
            .await
            .map_err(|e| RootCidError::Default(e.to_string()))?;
        println!("Reciept: {:?}", reciept);
        Ok(reciept)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RootCidError {
    #[error("eth client error: {0}")]
    EthClient(#[from] EthClientError),
    // Note: the web client never uses a signer
    #[cfg(not(target_arch = "wasm32"))]
    #[error("No signer")]
    MissingSigner,
    #[error("abi error: {0}")]
    Abi(#[from] ethers::abi::Error),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("default error: {0}")]
    Default(String),
}

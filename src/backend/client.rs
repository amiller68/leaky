use std::convert::TryFrom;
use std::ops::Deref;

use http::uri::Scheme;
use ipfs_api_backend_hyper::{IpfsClient as HyperIpfsClient, TryFromUri};

pub use ipfs_api_backend_hyper::request::Add as AddRequest;

use super::{IpfsError, IpfsRemote};

/// Default cid version to use when adding or hashing datat against the IPFS API
const DEFAULT_CID_VERSION: u32 = 1;
/// Default hash function to use when adding or hashing data against the IPFS API
const DEFAULT_HASH_FUNCTION: &str = "blake3";

/// Wrapper around a Hyper IPFS backend
#[derive(Default)]
pub struct IpfsClient(HyperIpfsClient);

impl TryFrom<IpfsRemote> for IpfsClient {
    type Error = IpfsError;

    fn try_from(remote: IpfsRemote) -> Result<Self, IpfsError> {
        let url = remote.api_url.clone();
        let scheme = Scheme::try_from(url.scheme())?;
        let username = url.username();
        let maybe_password = url.password();
        let host_str = url.host_str().unwrap();
        // TODO: for some reason, the port is not being parsed correctly, and is always None
        let port = url.port().unwrap_or(5001);
        let client = match maybe_password {
            Some(password) => HyperIpfsClient::from_host_and_port(scheme, host_str, port)?
                .with_credentials(username, password),
            None => HyperIpfsClient::from_host_and_port(scheme, host_str, port)?,
        };
        Ok(Self(client))
    }
}

impl Deref for IpfsClient {
    type Target = HyperIpfsClient;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(clippy::field_reassign_with_default)]
pub fn hash_data_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.pin = Some(false);
    add.cid_version = Some(DEFAULT_CID_VERSION);
    add.only_hash = Some(true);
    add.hash = Some(DEFAULT_HASH_FUNCTION);
    add
}

#[allow(clippy::field_reassign_with_default)]
pub fn add_data_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.cid_version = Some(DEFAULT_CID_VERSION);
    add.hash = Some(DEFAULT_HASH_FUNCTION);
    add
}

pub type IpfsClientError = ipfs_api_backend_hyper::Error;

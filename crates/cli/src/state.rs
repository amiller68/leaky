use std::convert::TryFrom;

use url::Url;

use super::Args;

pub struct AppState {}

impl TryFrom<&Args> for AppState {
    type Error = AppStateSetupError;

    fn try_from(args: &Args) -> Result<Self, Self::Error> {
        todo!()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateSetupError {
    #[error("invalid private key")]
    InvalidPrivateKey,
}

#[allow(dead_code)]
impl AppState {}

use leaky_common::prelude::*;

use std::path::PathBuf;

use async_trait::async_trait;

use crate::{AppState, Op};

#[derive(Debug, clap::Args, Clone)]
pub struct Init {
    #[clap(short, long)]
    input: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
}

#[async_trait]
impl Op for Init {
    type Error = InitError;
    type Output = Cid;

    async fn execute(&self, _state: &AppState) -> Result<Self::Output, Self::Error> {
        Ok(Cid::default())
    }
}

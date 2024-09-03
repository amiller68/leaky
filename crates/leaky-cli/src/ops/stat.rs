use async_trait::async_trait;

use leaky_common::prelude::*;

use crate::{AppState, Op};

#[derive(Debug, clap::Args, Clone)]
pub struct Stat;

#[derive(Debug, thiserror::Error)]
pub enum StatError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),
}

#[async_trait]
impl Op for Stat {
    type Error = StatError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let cid = state.cid().clone();
        let previous_cid = state.previous_cid().clone();
        let change_log = state.change_log().clone();
        
        println!("cid: {:?}", cid);
        println!("previousd cid: {:?}", previous_cid);
        println!("changes: {:?}", change_log);

        Ok(cid)
    }
}
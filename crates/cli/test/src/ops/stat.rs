use super::change_log::DisplayableChangeLog as ChangeLog;
use super::utils;

pub async fn stat() -> Result<ChangeLog, StatError> {
    let (_, change_log) = utils::load_on_disk().await?;
    Ok(ChangeLog(change_log))
}

#[derive(Debug, thiserror::Error)]
pub enum StatError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
}

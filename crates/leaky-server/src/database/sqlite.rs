use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::ConnectOptions;
use tracing::log::LevelFilter;
use url::Url;

use crate::database::DatabaseSetupError;

static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn connect_sqlite(url: &Url) -> Result<SqlitePool, DatabaseSetupError> {
    let connection_options = SqliteConnectOptions::from_url(url)
        .map_err(DatabaseSetupError::Unavailable)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(30))
        .log_statements(LevelFilter::Trace)
        .log_slow_statements(LevelFilter::Warn, Duration::from_millis(100))
        .statement_cache_capacity(2_500)
        .synchronous(SqliteSynchronous::Normal);

    SqlitePoolOptions::new()
        .idle_timeout(Duration::from_secs(30))
        .max_lifetime(Duration::from_secs(3600))
        .min_connections(2)
        .max_connections(32)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(connection_options)
        .await
        .map_err(DatabaseSetupError::Unavailable)
}

pub async fn migrate_sqlite(pool: &SqlitePool) -> Result<(), DatabaseSetupError> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(DatabaseSetupError::MigrationFailed)
}

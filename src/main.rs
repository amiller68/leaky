use std::fmt::Display;

use leaky::prelude::*;

mod cli;

use cli::{init, stage, Cli, Command, InitError, Parser, StageError};

#[tokio::main]
async fn main() {
    // Run the app and capture any errors
    capture_error(run().await);
}

pub async fn run() -> Result<(), AppError> {
    let args = Cli::parse();
    match args.command {
        Command::Init { ipfs_rpc } => {
            let cid = init(ipfs_rpc).await?;
            pretty_print(format!("LeakyBucket @ {:?}", cid));
        }
        Command::Stage => {
            let cid = stage().await?;
            pretty_print(format!("LeakyBucket @ {:?}", cid));
        }
        /*
                Command::Add { root, path } => {
                    leaky.pull(&root).await?;
                    // Read the data as a stream
                    let data = std::fs::read(&path)?;
                    let data = std::io::Cursor::new(data);

                    let cid = leaky.add(&path, data, None).await?;
                    pretty_print(&format!("{} -> {}", &path.to_string_lossy(), &cid));
                    changed = true;
                }
                Command::Ls { root, path } => {
                    leaky.pull(&root).await?;
                    let entries = leaky.ls(path).await?;
                    for entry in entries {
                        pretty_print(&format!("{} -> {}", entry.0, entry.0));
                    }
                }
        */
        _ => {}
    };
    /*
        if changed {
            leaky.push().await?;
            let cid = leaky.cid()?;
            pretty_print(format!("LeakyBucket @ {}", cid));
        }
    */
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("anyhow error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("Init error: {0}")]
    Init(#[from] InitError),
    #[error("Stage error: {0}")]
    Stage(#[from] StageError),
}

fn capture_error<T>(result: Result<T, AppError>) {
    match result {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}", e);
        }
    }
}

fn pretty_print<T: Display>(value: T) {
    let bullet = "•";
    println!("{} {}", bullet, value);
}

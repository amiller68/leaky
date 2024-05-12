use std::fmt::Display;

use url::Url;

mod cli;

use cli::ops::{add, init, push, stat, tag, AddError, InitError, PushError, StatError, TagError};
use cli::{Cli, Command, Parser};

#[tokio::main]
async fn main() {
    // Run the app and capture any errors
    capture_error(run().await);
}

pub async fn run() -> Result<(), AppError> {
    let args = Cli::parse();
    match args.command {
        Command::Init { maybe_ipfs_rpc_url } => {
            let ipfs_rpc = match maybe_ipfs_rpc_url {
                Some(url) => url,
                None => Url::parse("http://localhost:5001").unwrap(),
            };
            let cid = init(ipfs_rpc).await?;
            pretty_print(format!("LeakyBucket @ {}", cid));
        }
        Command::Add => {
            let cid = add().await?;
            pretty_print(format!("LeakyBucket @ {}", cid));
        }
        Command::Tag { path, metadata } => {
            let cid = tag(path, metadata).await?;
            pretty_print(format!("LeakyBucket @ {}", cid));
        }
        Command::Stat => {
            let stats = stat().await?;
            println!("{}", stats);
        }
        Command::Push => {
            let cid = push().await?;
            pretty_print(format!("LeakyBucket @ {}", cid));
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
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("anyhow error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("Init error: {0}")]
    Init(#[from] InitError),
    #[error("Stage error: {0}")]
    Add(#[from] AddError),
    #[error("Stat error: {0}")]
    Stat(#[from] StatError),
    #[error("Push error: {0}")]
    Push(#[from] PushError),
    #[error("Tag error: {0}")]
    Tag(#[from] TagError),
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

use std::path::PathBuf;

use clap::{command, Subcommand};
use url::Url;

pub use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Init {
        #[clap(long, short)]
        ipfs_rpc: Url,
    },
    Add,
    Stat,
    Push,
    Ls {
        #[clap(long, short)]
        path: PathBuf,
    },
    Cat {
        path: PathBuf,
    },
}
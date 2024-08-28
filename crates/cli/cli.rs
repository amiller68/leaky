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
        #[clap(long = "ipfs-rpc", short = 'i')]
        maybe_ipfs_rpc_url: Option<Url>,
        #[clap(long = "leaky-api", short = 'l')]
        maybe_leaky_api_url: Option<Url>,
    },
    Add,
    Tag {
        #[clap(long, short)]
        path: PathBuf,
        #[clap(long, short)]
        metadata: String,
    },
    Stat,
    Push,
    Pull,
    Ls {
        #[clap(long, short)]
        path: PathBuf,
    },
}

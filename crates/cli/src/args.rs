use std::error::Error;
use std::fmt::Display;

use clap::Subcommand;
use url::Url;

use super::ops::Init as InitOp;
use super::AppState;

pub use clap::Parser;

use std::fmt;

#[async_trait::async_trait]
pub trait Op: Send + Sync {
    type Error: Error + Send + Sync + 'static;
    type Output: Display;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error>;
}

#[macro_export]
macro_rules! command_enum {
    ($(($variant:ident, $type:ty)),* $(,)?) => {
        #[derive(Subcommand, Debug, Clone)]
        pub enum Command {
            $($variant($type),)*
        }

        #[derive(Debug)]
        pub enum OpOutput {
            $($variant(<$type as Op>::Output),)*
        }

        #[derive(Debug, thiserror::Error)]
        pub enum OpError {
            $(
                #[error(transparent)]
                $variant(<$type as Op>::Error),
            )*
        }

        #[async_trait::async_trait]
        impl Op for Command {
            type Output = OpOutput;
            type Error = OpError;

            async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
                match self {
                    $(
                        Command::$variant(op) => {
                            op.execute(state).await
                                .map(OpOutput::$variant)
                                .map_err(OpError::$variant)
                        },
                    )*
                }
            }
        }
    };
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[clap(long = "leaky-remote-url", short = 'i')]
    pub maybe_leaky_remote_url: Option<Url>,
    #[clap(long = "private-key", short = 'p')]
    pub maybe_private_key_hex: Option<String>,
}

use crate::command_enum;

command_enum! {
    (Init, InitOp),
    // Define more commands here
}

impl fmt::Display for OpOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            _ => unimplemented!(),
        }
    }
}

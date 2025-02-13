#![allow(dead_code)]
#![allow(clippy::result_large_err)]

use std::convert::TryFrom;

mod args;
mod change_log;
mod error;
mod ops;
mod state;
mod version;

use args::{Args, Op, Parser};
use change_log::ChangeLog;
use state::AppState;

#[tokio::main]
async fn main() {
    // Run the app and capture any errors
    let args = Args::parse();
    let state = match AppState::try_from(&args) {
        Ok(state) => state,
        Err(e) => {
            eprintln!("State error: {}", e);
            std::process::exit(1);
        }
    };

    let op = args.command.clone();
    match op.execute(&state).await {
        Ok(r) => {
            println!("{}", r);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Operation error: {:?}", e); // Print full error details
            std::process::exit(1);
        }
    };
}

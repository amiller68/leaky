#![allow(dead_code)]
// #![warn(missing_docs)]

use std::convert::TryFrom;

pub mod api;
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
    let state = AppState::try_from(&args).expect("valid state");
    let op = args.command.clone();
    match op.execute(&state).await {
        Ok(r) => println!("{}", r),
        Err(e) => {
            eprintln!("{}", e);
        }
    };
}

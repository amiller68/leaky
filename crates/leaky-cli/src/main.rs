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
    println!("Hello, world!");
    // Run the app and capture any errors
    let args = Args::parse();
    println!("args: {:?}", args);
    let state = AppState::try_from(&args).expect("valid state");
    println!("state init complete");
    let op = args.command.clone();
    println!("op: {:?}", op);
    match op.execute(&state).await {
        Ok(r) => println!("{}", r),
        Err(e) => {
            eprintln!("{}", e);
        }
    };
}

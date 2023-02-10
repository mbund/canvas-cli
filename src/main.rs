use clap::Parser;
use serde_derive::{Deserialize, Serialize};

pub mod auth;
pub mod submit;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    url: String,
    access_token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: "".into(),
            access_token: "".into(),
        }
    }
}

/// Interact with Canvas LMS from the command line
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand, Debug)]
enum Action {
    Auth(auth::AuthCommand),
    Submit(submit::SubmitCommand),
}

fn main() -> Result<(), anyhow::Error> {
    let mut cfg: Config = confy::load("canvas-cli", "config")?;

    let args = Args::parse();

    match args.action {
        Action::Auth(command) => command.action(&mut cfg),
        Action::Submit(command) => command.action(&cfg),
    }
}

use clap::Parser;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    version: u8,
    access_token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 0,
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

fn validate_access_token(token: &str) -> Result<String, String> {
    if token.trim().len() != token.len() {
        Err(String::from(
            "Token cannot have any leading or trailing whitespace",
        ))
    } else {
        Ok(token.to_owned())
    }
}

#[derive(clap::Subcommand, Debug)]
enum Action {
    /// Authenticate with Canvas
    Auth {
        #[arg(value_parser = validate_access_token)]
        /// Access token
        access_token: String,
    },
    Submit,
}

fn main() -> Result<(), confy::ConfyError> {
    let mut cfg: Config = confy::load("canvas-cli", None)?;
    println!("{:#?}", cfg);

    let args = Args::parse();
    match args.action {
        Action::Auth { access_token } => {
            println!("Authenticating with {}", access_token);
            cfg.access_token = access_token;
            confy::store("canvas-cli", None, &cfg)?;
        }
        Action::Submit => println!("Unimplemented"),
    }

    Ok(())
}

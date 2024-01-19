use clap::{CommandFactory, Parser, Subcommand};
use serde_derive::{Deserialize, Serialize};

pub mod auth;
pub mod download;
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

#[derive(Subcommand, Debug)]
enum Action {
    Auth(auth::AuthCommand),
    Submit(submit::SubmitCommand),
    Download(download::DownloadCommand),

    /// Generate shell completions
    Completions {
        /// The shell to generate the completions for
        #[arg(value_enum)]
        shell: clap_complete_command::Shell,
    },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    let mut cfg: Config = confy::load("canvas-cli", "config")?;

    let args = Args::parse();

    if let Ok(env_canvas_base_url) = std::env::var("CANVAS_BASE_URL") {
        cfg.url = env_canvas_base_url;
    }

    if let Ok(env_canvas_access_token) = std::env::var("CANVAS_ACCESS_TOKEN") {
        cfg.access_token = env_canvas_access_token;
    }

    match args.action {
        Action::Auth(command) => command.action(&mut cfg).await,
        Action::Submit(command) => command.action(&cfg).await,
        Action::Download(command) => command.action(&cfg).await,

        Action::Completions { shell } => Ok({
            shell.generate(&mut Args::command(), &mut std::io::stdout());
        }),
    }
}

use inquire::{Password, PasswordDisplayMode, Text};

use crate::Config;

fn validate_url(input: &str) -> Result<String, String> {
    match url::Url::parse(input) {
        Ok(url) => Ok(url.to_string()),
        Err(parse_error) => Err(parse_error.to_string()),
    }
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

#[derive(clap::Parser, Debug)]
/// Authenticate with Canvas
pub struct AuthCommand {
    #[arg(short, long, value_parser = validate_url)]
    /// URL for Canvas Instance, https://your.instructure.com
    url: Option<String>,

    #[arg(short, long, value_parser = validate_access_token)]
    /// Access token
    access_token: Option<String>,
}

impl AuthCommand {
    pub fn action(self, cfg: &mut Config) -> Result<(), anyhow::Error> {
        let url = match self.url {
            Some(url) => Ok(url),
            None => Text::new("Canvas Instance URL:").prompt(),
        }?;

        println!("Authenticating on {}", url);

        let access_token = match self.access_token {
            Some(access_token) => Ok(access_token),
            None => Password::new("Access token:")
                .with_display_mode(PasswordDisplayMode::Masked)
                .without_confirmation()
                .prompt(),
        }?;

        cfg.url = url;
        cfg.access_token = access_token;

        confy::store("canvas-cli", "config", cfg)?;

        Ok(())
    }
}

use indicatif::ProgressStyle;
use inquire::{Password, PasswordDisplayMode, Text};
use serde_derive::Deserialize;

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

#[derive(Deserialize, Debug)]
struct SelfResponse {
    name: String,
    pronouns: Option<String>,
}

impl AuthCommand {
    pub async fn action(self, cfg: &mut Config) -> Result<(), anyhow::Error> {
        let url = match self.url {
            Some(url) => Ok(url),
            None => Text::new("Canvas Instance URL:").prompt(),
        }?;

        let access_token = match self.access_token {
            Some(access_token) => Ok(access_token),
            None => Password::new("Access token:")
                .with_display_mode(PasswordDisplayMode::Masked)
                .without_confirmation()
                .prompt(),
        }?;

        let client = reqwest::Client::builder()
            .default_headers(
                std::iter::once((
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", access_token))
                        .unwrap(),
                ))
                .collect(),
            )
            .build()
            .unwrap();

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("Test query with authentication");

        let spinner_clone = spinner.clone();
        let spinner_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                spinner_clone.inc(1);
            }
        });

        let self_query = client
            .get(format!("{}/api/v1/users/self", url))
            .send()
            .await?
            .json::<SelfResponse>()
            .await?;
        spinner_task.abort();

        spinner.set_style(ProgressStyle::with_template("✓ {wide_msg}").unwrap());
        spinner.finish_with_message("Test query successful");
        println!("Authenticated as: ");
        match self_query.pronouns {
            Some(p) => println!("  {} ({})", self_query.name, p),
            None => println!("  {}", self_query.name),
        };

        cfg.url = url;
        cfg.access_token = access_token;

        // REST /api/v1/users/self

        confy::store("canvas-cli", "config", cfg)?;

        Ok(())
    }
}

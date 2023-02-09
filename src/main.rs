use std::error::Error;

use clap::Parser;
use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "generated/schema.json",
    query_path = "src/query_assignments.graphql",
    response_derives = "Debug"
)]
pub struct QueryAssignments;

#[derive(Debug)]
struct Assignment {
    name: String,
    id: String,
    due_at: Option<DateTime>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
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

#[derive(clap::Subcommand, Debug)]
enum Action {
    /// Authenticate with Canvas
    Auth {
        #[arg(value_parser = validate_url)]
        /// URL for Canvas Instance, https://your.instructure.com
        url: String,

        #[arg(value_parser = validate_access_token)]
        /// Access token
        access_token: String,
    },

    /// Submit Canvas assignment
    Submit {
        /// Name of assignment to submit to
        assignment_name: String,

        /// File(s)
        files: Vec<String>,
    },
}

fn main() -> Result<(), anyhow::Error> {
    let mut cfg: Config = confy::load("canvas-cli", "config")?;

    let args = Args::parse();
    match args.action {
        Action::Auth { url, access_token } => {
            println!("Authenticating on {}", url);
            cfg.url = url;
            cfg.access_token = access_token;

            confy::store("canvas-cli", "config", &cfg)?;
        }
        Action::Submit {
            assignment_name,
            files,
        } => {
            let client = reqwest::blocking::Client::builder()
                .default_headers(
                    std::iter::once((
                        reqwest::header::AUTHORIZATION,
                        reqwest::header::HeaderValue::from_str(&format!(
                            "Bearer {}",
                            cfg.access_token
                        ))
                        .unwrap(),
                    ))
                    .collect(),
                )
                .build()?;

            // find assignment
            let queried_assignments =
                graphql_client::reqwest::post_graphql_blocking::<QueryAssignments, _>(
                    &client,
                    cfg.url + "/api/graphql",
                    query_assignments::Variables {},
                )
                .unwrap();

            println!("{:#?}", queried_assignments);

            let assignments: Vec<Assignment> = queried_assignments
                .data
                .unwrap()
                .all_courses
                .into_iter()
                .flat_map(|x| {
                    x.into_iter().flat_map(|y| {
                        y.assignments_connection
                            .unwrap()
                            .nodes
                            .unwrap()
                            .into_iter()
                            .map(|z| {
                                let w = z.unwrap();
                                Assignment {
                                    name: w.name.unwrap(),
                                    id: w.id,
                                    due_at: w.due_at.clone(),
                                }
                            })
                    })
                })
                .collect();

            println!("{:#?}", assignments);

            // verify files exist
            for file in files {
                std::fs::metadata(&file)?;
            }

            // submit files
        }
    }

    Ok(())
}

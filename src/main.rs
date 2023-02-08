use std::error::Error;

use clap::Parser;
use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "generated/schema.json",
    query_path = "src/query_assignments.graphql"
)]
pub struct QueryAssignments;

// async fn perform_my_query(variables: query_assignments::Variables) -> Result<(), Box<dyn Error>> {
//     // this is the important line
//     let request_body = QueryAssignments::build_query(variables);

//     let client = reqwest::Client::new();
//     let mut res = client
//         .post("/api/graphql")
//         .json(&request_body)
//         .send()
//         .await?;
//     let response_body: Response<query_assignments::ResponseData> = res.json().await?;
//     println!("{:#?}", response_body);
//     Ok(())
// }

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

fn main() {
    let mut cfg: Config = match confy::load("canvas-cli", "config") {
        Ok(cfg) => cfg,
        Err(error) => panic!("Problem loading config file: {:?}", error),
    };

    let args = Args::parse();
    match args.action {
        Action::Auth { url, access_token } => {
            println!("Authenticating on {}", url);
            cfg.url = url;
            cfg.access_token = access_token;

            match confy::store("canvas-cli", "config", &cfg) {
                Ok(cfg) => cfg,
                Err(error) => panic!("Problem writing config file: {:?}", error),
            };
        }
        Action::Submit {
            assignment_name,
            files,
        } => {
            // find assignment

            // verify files exist
            for file in files {
                match std::fs::metadata(&file) {
                    Ok(_metadata) => {}
                    Err(error) => {
                        panic!("Problem fetching file metadata on {:?}: {:?}", file, error)
                    }
                };
            }

            // submit files
        }
    }

    // Ok(())
}

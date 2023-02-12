#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::Hash,
};

use crate::{submit::query_assignments::SubmissionType, Config};
use anyhow::anyhow;
use colored::Colorize;
use fuzzy_matcher::FuzzyMatcher;
use graphql_client::GraphQLQuery;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::*;
use reqwest::{
    multipart::{Form, Part},
    Body, Client,
};
use serde::Deserialize;
use tokio_util::codec::{BytesCodec, FramedRead};

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "generated/schema.json",
    query_path = "src/query_assignments.graphql",
    response_derives = "Debug, Clone"
)]
pub struct QueryAssignments;

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
struct Course {
    name: String,
    id: String,
    favorite: bool,
    css_color: String,
}

impl Display for Course {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let color = csscolorparser::parse(&self.css_color)
            .unwrap()
            .to_linear_rgba_u8();
        write!(
            f,
            "{}{}{}",
            "█ ".truecolor(color.0, color.1, color.2),
            self.name,
            if self.favorite { " ★" } else { "" }.yellow()
        )
    }
}

#[derive(Debug)]
struct Assignment {
    name: String,
    id: String,
    due_at: Option<DateTime>,
    course: Course,
    submitted: bool,
    submission_types: Vec<SubmissionType>,
}

impl Display for Assignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, if self.submitted { " ✓" } else { "" })
    }
}

#[derive(clap::Parser, Debug)]
/// Submit Canvas assignment
pub struct SubmitCommand {
    /// File(s)
    files: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct FavoritesResponse {
    id: u32,
    name: String,
}

#[derive(Deserialize, Debug)]
struct ColorsResponse {
    custom_colors: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct UploadBucket {
    upload_url: String,
    upload_params: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct UploadResponse {
    id: u32,
    url: String,
    content_type: Option<String>,
    display_name: Option<String>,
    size: Option<u32>,
}

impl SubmitCommand {
    pub async fn action(&self, cfg: &Config) -> Result<(), anyhow::Error> {
        // verify all files exist first before doing anything which needs a network connections
        if self.files.len() == 0 {
            Err(anyhow!("Must submit at least one file"))?;
        }

        for file in self.files.iter() {
            match std::fs::metadata(&file) {
                Ok(_) => Ok(()),
                Err(error) => Err(anyhow!("{}: {}", error, file)),
            }?;

            log::info!("Verified file exists: {}", file);
        }

        log::info!("Verified all files exist");
        println!("✓ Verified all files exist");

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("Querying assignment information");

        let spinner_clone = spinner.clone();
        let spinner_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                spinner_clone.inc(1);
            }
        });

        let url = cfg.url.to_owned();
        let access_token = cfg.access_token.to_owned();

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

        let graphql_assignment_request = graphql_client::reqwest::post_graphql::<QueryAssignments, _>(
            &client,
            format!("{}api/graphql", url),
            query_assignments::Variables {},
        );

        let favorites_request = client
            .get(format!("{}api/v1/users/self/favorites/courses", url))
            .send();

        let colors_request = client
            .get(format!("{}api/v1/users/self/colors", url))
            .send();

        let colors = colors_request.await?.json::<ColorsResponse>().await?;
        log::info!("Made REST request to get course colors");

        let favorites = favorites_request
            .await?
            .json::<Vec<FavoritesResponse>>()
            .await?;
        log::info!("Made REST request to get favorite courses");

        let queried_assignments = graphql_assignment_request.await?.data.unwrap();
        log::info!("Made GraphQL request to get all courses and assignments");

        spinner_task.abort();

        spinner.set_style(ProgressStyle::with_template("✓ {wide_msg}").unwrap());
        spinner.finish_with_message("Queried assignment information");

        // translate graphql response into one long list of assignments
        let mut assignments: Vec<Assignment> = queried_assignments
            .all_courses
            .iter()
            .flat_map(|all_courses| {
                all_courses.iter().flat_map(|course| {
                    course
                        .assignments_connection
                        .as_ref()
                        .unwrap()
                        .nodes
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|assignment| {
                            let assignment_props = assignment.as_ref().unwrap();

                            let submissions_nodes = assignment_props
                                .submissions_connection
                                .as_ref()
                                .unwrap()
                                .nodes
                                .as_ref()
                                .unwrap();

                            let submitted = if submissions_nodes.len() > 0 {
                                !submissions_nodes
                                    .iter()
                                    .next()
                                    .as_ref()
                                    .unwrap()
                                    .as_ref()
                                    .unwrap()
                                    .submission_status
                                    .as_ref()
                                    .unwrap()
                                    .is_empty()
                            } else {
                                false
                            };

                            let submission_types =
                                assignment_props.submission_types.as_ref().unwrap();

                            Assignment {
                                name: assignment_props.name.as_ref().unwrap().to_owned(),
                                id: assignment_props.id.to_owned(),
                                due_at: assignment_props.due_at.to_owned(),
                                submitted,
                                submission_types: submission_types.to_owned(),
                                course: Course {
                                    name: course.name.to_owned(),
                                    id: course.id.to_owned(),
                                    favorite: favorites.iter().any(|favorite| {
                                        favorite.id == course.id.parse::<u32>().unwrap()
                                    }),
                                    css_color: colors
                                        .custom_colors
                                        .get(course.asset_string.as_ref().unwrap())
                                        .unwrap()
                                        .to_string(),
                                },
                            }
                        })
                })
            })
            .collect();

        // get courses
        let mut courses: Vec<Course> = assignments
            .iter()
            .map(|assignment| assignment.course.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        courses.sort_by(|a, b| b.favorite.cmp(&a.favorite).then(a.name.cmp(&b.name)));
        let course = Select::new("Course?", courses).prompt()?;

        // get assignment
        assignments.retain(|assignment| {
            assignment.submission_types.iter().any(|submission_type| {
                match submission_type {
                    SubmissionType::online_upload => true,
                    _ => false, // TODO: support more upload types
                }
            })
        });
        assignments.retain(|assignment| assignment.course == course);
        assignments.sort_by(|a, b| a.submitted.cmp(&b.submitted).then(a.due_at.cmp(&b.due_at)));
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
        let assignment = Select::new("Assignment?", assignments)
            .with_filter(&|input, _, string_value, _| {
                matcher.fuzzy_match(string_value, input).is_some()
            })
            .prompt()?;

        // upload files
        let multi_progress = MultiProgress::new();
        let futures = self.files.iter().map(|filepath| {
            upload_file(
                &url,
                &course,
                &assignment,
                &client,
                &filepath,
                &multi_progress,
            )
        });

        let uploaded_files = futures::future::join_all(futures).await;
        let mut params: Vec<(String, String)> = uploaded_files
            .into_iter()
            .map(|f| {
                (
                    "submission[file_ids][]".to_string(),
                    f.unwrap().id.to_string(),
                )
            })
            .collect();
        params.push((
            "submission[submission_type]".to_string(),
            "online_upload".to_string(),
        ));
        let submit_reponse = client
            .post(format!(
                "{}api/v1/courses/{}/assignments/{}/submissions",
                url, course.id, assignment.id
            ))
            .query(&params)
            .send()
            .await?;

        submit_reponse.error_for_status()?;

        println!(
            "✓ Successfully submitted file{} to assignment 🎉",
            if self.files.len() > 1 { "s" } else { "" }
        );

        Ok(())
    }
}

async fn upload_file(
    url: &str,
    course: &Course,
    assignment: &Assignment,
    client: &Client,
    filepath: &str,
    multi_progress: &MultiProgress,
) -> Result<UploadResponse, anyhow::Error> {
    let metadata = std::fs::metadata(filepath).unwrap();
    let path = std::path::Path::new(filepath);
    let file = tokio::fs::File::open(path).await.unwrap();
    let basename = path.file_name().unwrap().to_str().unwrap();

    let spinner = multi_progress.add(ProgressBar::new_spinner());
    spinner.set_message(format!("Uploading file {} as {}", filepath, basename));

    let spinner_clone = spinner.clone();
    let spinner_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            spinner_clone.inc(1);
        }
    });

    let upload_bucket = client
        .post(format!(
            "{}api/v1/courses/{}/assignments/{}/submissions/self/files",
            url, course.id, assignment.id
        ))
        .form(&HashMap::from([
            ("name", basename),
            ("size", metadata.len().to_string().as_str()),
        ]))
        .send()
        .await?
        .json::<UploadBucket>()
        .await
        .unwrap();

    spinner.set_message(format!(
        "Uploading {}: recieved upload bucket, sending file payload",
        filepath
    ));

    let location = client
        .post(upload_bucket.upload_url)
        .multipart(
            upload_bucket
                .upload_params
                .into_iter()
                .fold(Form::new(), |form, (k, v)| form.text(k, v))
                .part(
                    "file",
                    Part::stream(Body::wrap_stream(FramedRead::new(file, BytesCodec::new()))),
                ),
        )
        .send()
        .await?
        .headers()
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    spinner.set_message(format!(
        "Uploading {}: recieved upload location, checking response",
        filepath
    ));

    let upload_response = client
        .post(location)
        .header("Content-Length", 0)
        .send()
        .await?
        .json::<UploadResponse>()
        .await
        .unwrap();

    spinner_task.abort();
    spinner.set_style(ProgressStyle::with_template("✓ {wide_msg}").unwrap());
    match &upload_response.display_name {
        Some(display_name) => {
            spinner.finish_with_message(format!("Uploaded file {} as {}", filepath, display_name))
        }
        None => spinner.finish_with_message(format!("Uploaded file {}", filepath)),
    }

    Ok(upload_response)
}

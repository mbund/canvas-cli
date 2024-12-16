use std::{collections::HashMap, fmt::Display};

use crate::{Config, NonEmptyConfig};
use anyhow::anyhow;
use canvas_cli::{Course, DateTime};
use fuzzy_matcher::FuzzyMatcher;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::Select;
use regex::Regex;
use reqwest::{
    multipart::{Form, Part},
    Body, Client,
};
use serde_derive::Deserialize;
use tokio_util::codec::{BytesCodec, FramedRead};

#[derive(Debug)]
struct Assignment {
    id: u32,
    name: String,
    due_at: Option<DateTime>,
    is_graded: bool,
}

impl Display for Assignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, if self.is_graded { " ✓" } else { "" })
    }
}

#[derive(Deserialize, Debug)]
struct AssignmentResponse {
    id: u32,
    name: String,
    due_at: Option<DateTime>,
    locked_for_user: bool,
    graded_submissions_exist: bool,
    submission_types: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct UploadBucket {
    upload_url: String,
    upload_params: HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct UploadResponse {
    id: u32,
    display_name: Option<String>,
}

#[derive(clap::Parser, Debug)]
/// Submit Canvas assignment
pub struct SubmitCommand {
    /// File(s)
    files: Vec<String>,

    /// Canvas URL to parse
    #[clap(long, short)]
    url: Option<String>,

    /// Canvas course ID
    #[clap(long, short)]
    course: Option<u32>,

    /// Canvas assignment ID
    #[clap(long, short)]
    assignment: Option<u32>,
}

impl SubmitCommand {
    pub async fn action(&self, cfg: &Config) -> Result<(), anyhow::Error> {
        let NonEmptyConfig {
            url: mut base_url,
            access_token,
        } = cfg.ensure_non_empty()?;

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

        println!("✓ Verified all files exist");

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

        let mut course_id = self.course;
        let mut assignment_id = self.assignment;
        let canvas_assignment_url = if let Ok(env_canvas_url) = std::env::var("CANVAS_URL") {
            Some(env_canvas_url)
        } else {
            self.url.clone()
        };

        if let Some(canvas_assignment_url) = canvas_assignment_url {
            let regex = Regex::new(r#"(https://.+)/courses/(\d+)(?:/assignments/(\d+))?"#).unwrap();

            let captures = regex.captures(&canvas_assignment_url).unwrap();
            base_url = captures.get(1).unwrap().as_str().to_string();
            course_id = Some(captures.get(2).unwrap().as_str().parse::<u32>().unwrap());
            if let Some(a_id) = captures.get(3) {
                assignment_id = Some(a_id.as_str().parse::<u32>().unwrap());
            }
        }

        if let Ok(env_canvas_course_id) = std::env::var("CANVAS_COURSE_ID") {
            course_id = Some(env_canvas_course_id.parse::<u32>().unwrap())
        }

        if let Ok(env_canvas_assignment_id) = std::env::var("CANVAS_ASSIGNMENT_ID") {
            assignment_id = Some(env_canvas_assignment_id.parse::<u32>().unwrap())
        }

        let base_url = base_url;
        let course_id = course_id;
        let assignment_id = assignment_id;

        let course = Course::fetch(course_id, &base_url, &client).await?;

        log::info!("Selected course {}", course.id);

        let assignment = if let Some(assignment_id) = assignment_id {
            let assignment_response = client
                .get(format!(
                    "{}/api/v1/courses/{}/assignments/{}",
                    base_url, course.id, assignment_id
                ))
                .send()
                .await?
                .json::<AssignmentResponse>()
                .await?;
            log::info!("Made REST request to get assignment information");

            let assignment = Assignment {
                name: assignment_response.name,
                id: assignment_response.id,
                due_at: assignment_response.due_at,
                is_graded: assignment_response.graded_submissions_exist,
            };

            println!("✓ Found {assignment}");

            assignment
        } else {
            let mut assignments: Vec<Assignment> = client
                .get(format!(
                    "{}/api/v1/courses/{}/assignments?per_page=1000",
                    base_url, course.id
                ))
                .send()
                .await?
                .json::<Vec<AssignmentResponse>>()
                .await?
                .into_iter()
                .filter(|assignment| {
                    !assignment.locked_for_user && assignment.submission_types[0] == "online_upload"
                })
                .map(|assignment| Assignment {
                    name: assignment.name,
                    id: assignment.id,
                    due_at: assignment.due_at,
                    is_graded: assignment.graded_submissions_exist,
                })
                .collect();
            log::info!("Made REST request to get assignment information");
            println!("✓ Queried assignment information");

            assignments.sort_by(|a, b| a.is_graded.cmp(&b.is_graded).then(a.due_at.cmp(&b.due_at)));
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            Select::new("Assignment?", assignments)
                .with_filter(&|input, _, string_value, _| {
                    matcher.fuzzy_match(string_value, input).is_some()
                })
                .prompt()?
        };

        log::info!("Selected assignment {}", assignment.id);

        let multi_progress = MultiProgress::new();
        let future_files = self.files.iter().map(|filepath| {
            upload_file(
                &base_url,
                &course,
                &assignment,
                &client,
                &filepath,
                &multi_progress,
            )
        });

        let uploaded_files = futures::future::join_all(future_files).await;
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
                "{}/api/v1/courses/{}/assignments/{}/submissions",
                base_url, course.id, assignment.id
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
            "{}/api/v1/courses/{}/assignments/{}/submissions/self/files",
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

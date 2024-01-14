#![allow(dead_code)]

use std::{collections::HashMap, fmt::Display, hash::Hash};

use crate::Config;
use anyhow::anyhow;
use colored::Colorize;
use fuzzy_matcher::FuzzyMatcher;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::*;
use reqwest::{
    multipart::{Form, Part},
    Body, Client,
};
use serde_derive::Deserialize;
use tokio_util::codec::{BytesCodec, FramedRead};

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
struct Course {
    name: String,
    id: u32,
    is_favorite: bool,
    css_color: Option<String>,
    created_at: DateTime,
}

impl Display for Course {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let css_color = self.css_color.clone().unwrap_or("#000000".to_string());
        let color = csscolorparser::parse(&css_color)
            .unwrap()
            .to_linear_rgba_u8();
        write!(
            f,
            "{}{}{}",
            "█ ".truecolor(color.0, color.1, color.2),
            self.name,
            if self.is_favorite { " ★" } else { "" }.yellow()
        )
    }
}

#[derive(Debug)]
struct Assignment {
    name: String,
    id: u32,
    due_at: Option<DateTime>,
    is_graded: bool,
}

impl Display for Assignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, if self.is_graded { " ✓" } else { "" })
    }
}

#[derive(clap::Parser, Debug)]
/// Submit Canvas assignment
pub struct SubmitCommand {
    /// File(s)
    files: Vec<String>,

    /// Course ID.
    /// If not specified, will prompt for course
    #[clap(long, short)]
    course: Option<u32>,

    /// Assignment ID.
    /// If not specified, will prompt for assignment
    #[clap(long, short)]
    assignment: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct CourseResponse {
    id: u32,
    name: String,
    is_favorite: bool,
    created_at: DateTime,
    concluded: bool,
}

#[derive(Deserialize, Debug)]
struct AssignmentResponse {
    name: String,
    id: u32,
    due_at: Option<DateTime>,
    locked_for_user: bool,
    graded_submissions_exist: bool,
    submission_types: Vec<String>,
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

        println!("✓ Verified all files exist");

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

        let course = if let Some(course_id) = self.course {
            let course_response = client
                .get(format!(
                    "{}/api/v1/courses/{}?include[]=favorites&include[]=concluded",
                    url, course_id
                ))
                .send()
                .await?
                .json::<CourseResponse>()
                .await?;
            log::info!("Made REST request to get course information");

            let course_colors: HashMap<u32, String> = client
                .get(format!("{}/api/v1/users/self/colors", url))
                .send()
                .await?
                .json::<ColorsResponse>()
                .await?
                .custom_colors
                .into_iter()
                .filter(|(k, _)| k.starts_with("course_"))
                .map(|(k, v)| (k.trim_start_matches("course_").parse::<u32>().unwrap(), v))
                .collect();
            log::info!("Made REST request to get course colors");

            println!("✓ Queried course information");

            let course = Course {
                name: course_response.name,
                id: course_response.id,
                is_favorite: course_response.is_favorite,
                css_color: course_colors.get(&course_response.id).cloned(),
                created_at: course_response.created_at,
            };

            println!("✓ Found {course}");
            course
        } else {
            let courses_response = client
                .get(format!(
                    "{}/api/v1/courses?per_page=1000&include[]=favorites&include[]=concluded",
                    url
                ))
                .send()
                .await?
                .json::<Vec<CourseResponse>>()
                .await?;
            log::info!("Made REST request to get favorite courses");

            let course_colors: HashMap<u32, String> = client
                .get(format!("{}/api/v1/users/self/colors", url))
                .send()
                .await?
                .json::<ColorsResponse>()
                .await?
                .custom_colors
                .into_iter()
                .filter(|(k, _)| k.starts_with("course_"))
                .map(|(k, v)| (k.trim_start_matches("course_").parse::<u32>().unwrap(), v))
                .collect();
            log::info!("Made REST request to get course colors");

            println!("✓ Queried course information");

            let mut courses: Vec<Course> = courses_response
                .into_iter()
                .filter(|course| !course.concluded)
                .map(|course| Course {
                    name: course.name.clone(),
                    id: course.id,
                    is_favorite: course.is_favorite,
                    css_color: course_colors.get(&course.id).cloned(),
                    created_at: course.created_at,
                })
                .collect();

            courses.sort_by(|a, b| {
                b.is_favorite
                    .cmp(&a.is_favorite)
                    .then(a.created_at.cmp(&b.created_at))
            });
            Select::new("Course?", courses).prompt()?
        };

        log::info!("Selected course {}", course.id);

        let assignment = if let Some(assignment_id) = self.assignment {
            let assignment_response = client
                .get(format!(
                    "{}/api/v1/courses/{}/assignments/{}",
                    url, course.id, assignment_id
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
                    url, course.id
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
                &url,
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

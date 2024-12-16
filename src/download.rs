use std::{fmt::Display, fs, io::Cursor, path::PathBuf};

use crate::{Config, NonEmptyConfig};
use canvas_cli::{Course, DateTime};
use fuzzy_matcher::FuzzyMatcher;
use human_bytes::human_bytes;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::MultiSelect;
use regex::Regex;
use serde_derive::Deserialize;

#[derive(Debug)]
struct File {
    id: u32,
    filename: String,
    url: String,
    size: u32,
    updated_at: DateTime,
}

impl Display for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.filename, human_bytes(self.size))
    }
}

#[derive(Deserialize, Debug)]
struct FileResponse {
    id: u32,
    filename: String,
    url: String,
    size: u32,
    updated_at: DateTime,
}

#[derive(clap::Parser, Debug)]
/// Download files from a course
pub struct DownloadCommand {
    /// Canvas course ID
    #[clap(long, short)]
    course: Option<u32>,

    /// Canvas URL to parse
    #[clap(long, short)]
    url: Option<String>,

    /// Canvas file IDs
    #[clap(value_parser, num_args = 1.., value_delimiter = ' ')]
    files: Option<Vec<u32>>,

    /// Output directory
    #[clap(long, short)]
    directory: Option<PathBuf>,
}

impl DownloadCommand {
    pub async fn action(&self, cfg: &Config) -> Result<(), anyhow::Error> {
        let NonEmptyConfig {
            url: mut base_url,
            access_token,
        } = cfg.ensure_non_empty()?;

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
        let canvas_file_url = if let Ok(env_canvas_url) = std::env::var("CANVAS_URL") {
            Some(env_canvas_url)
        } else {
            self.url.clone()
        };

        if let Some(canvas_assignment_url) = canvas_file_url {
            let regex = Regex::new(r#"(https://.+)/courses/(\d+)"#).unwrap();

            let captures = regex.captures(&canvas_assignment_url).unwrap();
            base_url = captures.get(1).unwrap().as_str().to_string();
            course_id = Some(captures.get(2).unwrap().as_str().parse::<u32>().unwrap());
        }

        if let Ok(env_canvas_course_id) = std::env::var("CANVAS_COURSE_ID") {
            course_id = Some(env_canvas_course_id.parse::<u32>().unwrap())
        }

        let base_url = base_url;
        let course_id = course_id;

        let course = Course::fetch(course_id, &base_url, &client).await?;

        log::info!("Selected course {}", course.id);

        let file_request = client
            .get(format!(
                "{}/api/v1/courses/{}/files?per_page=1000",
                base_url, course.id
            ))
            .send()
            .await?;

        if !file_request.status().is_success() {
            println!("No files available");
            return Ok(());
        }

        let mut files: Vec<File> = file_request
            .json::<Vec<FileResponse>>()
            .await?
            .into_iter()
            .map(|file| File {
                id: file.id,
                filename: file.filename,
                url: file.url,
                size: file.size,
                updated_at: file.updated_at,
            })
            .collect();

        if files.len() == 0 {
            println!("No files available");
            return Ok(());
        }

        let files = if let Some(file_ids) = &self.files {
            println!("✓ Queried all files");
            files.retain(|file| file_ids.contains(&file.id));
            files
        } else {
            files.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            let files = MultiSelect::new("Files?", files)
                .with_filter(&|input, _, string_value, _| {
                    matcher.fuzzy_match(string_value, input).is_some()
                })
                .prompt()?;
            files
        };

        if files.len() == 0 {
            println!("No files selected");
            return Ok(());
        }

        if let Some(directory) = &self.directory {
            fs::create_dir_all(directory)?;
            println!(
                "✓ Will download files into {}",
                directory.canonicalize()?.display()
            );
        }

        let multi_progress = MultiProgress::new();
        let future_files = files
            .iter()
            .map(|file| upload_file(&file, self.directory.as_ref(), &multi_progress));
        futures::future::join_all(future_files).await;

        println!("✓ Successfully downloaded files 🎉");

        Ok(())
    }
}

async fn upload_file(
    file: &File,
    directory: Option<&PathBuf>,
    multi_progress: &MultiProgress,
) -> Result<(), anyhow::Error> {
    let spinner = multi_progress.add(ProgressBar::new_spinner());
    spinner.set_message(format!("Downloading file {}", file));

    let spinner_clone = spinner.clone();
    let spinner_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            spinner_clone.inc(1);
        }
    });

    let path = if let Some(directory) = directory {
        directory.join(&file.filename)
    } else {
        PathBuf::from(&file.filename)
    };

    let response = reqwest::get(&file.url).await?;
    let mut fsfile = std::fs::File::create(path)?;
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut fsfile)?;

    spinner_task.abort();
    spinner.set_style(ProgressStyle::with_template("✓ {wide_msg}").unwrap());
    spinner.finish_with_message(format!("Downloaded file {}", file));

    Ok(())
}

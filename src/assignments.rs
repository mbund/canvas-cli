use crate::{Config, NonEmptyConfig};
use chrono::{DateTime, Utc};
use colored::Colorize;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// Assignment data structure matching Canvas API response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Assignment {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "due_at")]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(rename = "points_possible")]
    pub points_possible: Option<f64>,
    #[serde(rename = "submission_types", default)]
    pub submission_types: Vec<String>,
    #[serde(rename = "workflow_state", default)]
    pub workflow_state: String,
    #[serde(rename = "html_url", default)]
    pub html_url: String,
    pub submission: Option<SubmissionInfo>,
    #[serde(rename = "assignment_group_id")]
    pub assignment_group_id: Option<u32>,
    #[serde(default)]
    pub locked: bool,
    #[serde(rename = "lock_info")]
    pub lock_info: Option<LockInfo>,
    #[serde(rename = "lock_at")]
    pub lock_at: Option<DateTime<Utc>>,
    #[serde(rename = "unlock_at")]
    pub unlock_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmissionInfo {
    pub body: Option<String>,
    #[serde(rename = "submitted_at")]
    pub submitted_at: Option<DateTime<Utc>>,
    pub grade: Option<String>,
    pub score: Option<f64>,
    #[serde(rename = "workflow_state")]
    pub workflow_state: String,
    pub attempt: Option<u32>,
    #[serde(rename = "submission_type")]
    pub submission_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LockInfo {
    #[serde(rename = "lock_at")]
    pub lock_at: Option<DateTime<Utc>>,
    #[serde(rename = "unlock_at")]
    pub unlock_at: Option<DateTime<Utc>>,
    #[serde(rename = "can_view")]
    pub can_view: bool,
}

/// Output format options
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Markdown,
    Csv,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Markdown => write!(f, "markdown"),
            OutputFormat::Csv => write!(f, "csv"),
        }
    }
}

#[derive(clap::Parser, Debug)]
/// View assignment information for a course
pub struct AssignmentsCommand {
    /// Canvas course ID
    #[clap(long, short)]
    course: Option<u32>,

    /// Canvas course or assignment URL to parse
    #[clap(long, short)]
    url: Option<String>,

    /// Specific assignment ID (shows only that assignment)
    #[clap(long, short)]
    assignment: Option<u32>,

    /// Output format
    #[clap(long, short = 'f', default_value = "table", value_enum)]
    format: OutputFormat,

    /// Filter by assignment type (e.g., "online_upload", "online_text_entry")
    #[clap(long = "type")]
    type_filter: Option<String>,

    /// Show only upcoming assignments (due in the future)
    #[clap(long)]
    upcoming: bool,

    /// Show only incomplete assignments (not yet submitted)
    #[clap(long)]
    incomplete: bool,

    /// Show only missing or late assignments
    #[clap(long)]
    missing: bool,

    /// Include assignment description in output
    #[clap(long, short)]
    verbose: bool,

    /// Limit number of assignments shown (0 for all)
    #[clap(long, default_value = "0")]
    limit: usize,
}

impl AssignmentsCommand {
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
        let mut assignment_id = self.assignment;

        // Parse URL if provided
        if let Some(canvas_url) = &self.url {
            let regex =
                Regex::new(r#"(https://.+)/courses/(\d+)(?:/assignments/(\d+))?"#).unwrap();
            if let Some(captures) = regex.captures(canvas_url) {
                base_url = captures.get(1).unwrap().as_str().to_string();
                course_id = Some(captures.get(2).unwrap().as_str().parse::<u32>().unwrap());
                if let Some(assignment_match) = captures.get(3) {
                    assignment_id = Some(assignment_match.as_str().parse::<u32>().unwrap());
                }
            }
        }

        // Check environment variables
        if let Ok(env_course_id) = std::env::var("CANVAS_COURSE_ID") {
            course_id = Some(env_course_id.parse::<u32>()?);
        }

        let course_id = course_id.ok_or_else(|| {
            anyhow::anyhow!(
                "Course ID required. Use --course, --url, or set CANVAS_COURSE_ID environment variable."
            )
        })?;

        // Fetch course information
        let course = crate::Course::fetch(Some(course_id), &base_url, &client).await?;

        // Fetch assignments
        let mut assignments = Assignment::fetch_all(course_id, &base_url, &client).await?;

        // Filter by specific assignment if requested
        if let Some(assignment_id) = assignment_id {
            assignments.retain(|a| a.id == assignment_id);
            if assignments.is_empty() {
                anyhow::bail!("Assignment {} not found in course {}", assignment_id, course_id);
            }
        }

        // Apply filters
        let now = Utc::now();

        if self.upcoming {
            assignments.retain(|a| a.due_at.map(|d| d > now).unwrap_or(false));
        }

        if self.incomplete {
            assignments.retain(|a| {
                a.submission
                    .as_ref()
                    .map(|s| s.workflow_state != "submitted" && s.workflow_state != "graded")
                    .unwrap_or(true)
            });
        }

        if self.missing {
            assignments.retain(|a| {
                a.submission
                    .as_ref()
                    .map(|s| s.workflow_state == "missing" || s.workflow_state == "late")
                    .unwrap_or(false)
            });
        }

        if let Some(type_filter) = &self.type_filter {
            assignments.retain(|a| {
                a.submission_types
                    .iter()
                    .any(|t| t.to_lowercase().contains(&type_filter.to_lowercase()))
            });
        }

        // Sort by due date (earliest first), with null dates at the end
        assignments.sort_by(|a, b| {
            a.due_at
                .cmp(&b.due_at)
                .then_with(|| a.name.cmp(&b.name))
        });

        // Apply limit
        if self.limit > 0 && assignments.len() > self.limit {
            assignments.truncate(self.limit);
        }

        // Output based on format
        match self.format {
            OutputFormat::Json => {
                self.output_json(&assignments, course_id, &course.name)?;
            }
            OutputFormat::Markdown => {
                self.output_markdown(&assignments)?;
            }
            OutputFormat::Csv => {
                self.output_csv(&assignments)?;
            }
            OutputFormat::Table => {
                self.output_table(&assignments)?;
            }
        }

        Ok(())
    }

    fn output_json(
        &self,
        assignments: &[Assignment],
        course_id: u32,
        course_name: &str,
    ) -> Result<(), anyhow::Error> {
        #[derive(Serialize)]
        struct AssignmentsOutput {
            course_id: u32,
            course_name: String,
            count: usize,
            fetched_at: DateTime<Utc>,
            assignments: Vec<Assignment>,
        }

        let output = AssignmentsOutput {
            course_id,
            course_name: course_name.to_string(),
            count: assignments.len(),
            fetched_at: Utc::now(),
            assignments: assignments.to_vec(),
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }

    fn output_markdown(&self, assignments: &[Assignment]) -> Result<(), anyhow::Error> {
        if assignments.is_empty() {
            println!("No assignments found.");
            return Ok(());
        }

        // Header
        println!("| ID | Name | Due Date | Points | Status |");
        println!("|----|------|----------|--------|--------|");

        // Rows
        for assignment in assignments {
            let due_date = assignment
                .due_at
                .map(|d| d.format("%b %d, %Y %H:%M").to_string())
                .unwrap_or_else(|| "No due date".to_string());

            let points = assignment
                .points_possible
                .map(|p| p.to_string())
                .unwrap_or_else(|| "—".to_string());

            let status = match &assignment.submission {
                Some(sub) => match sub.workflow_state.as_str() {
                    "submitted" => "✓ Submitted".to_string(),
                    "graded" => format!("✓ Graded ({})", sub.grade.clone().unwrap_or_default()),
                    "missing" => "❌ Missing".to_string(),
                    "late" => "⏰ Late".to_string(),
                    _ => sub.workflow_state.clone(),
                },
                None => "📝 Not submitted".to_string(),
            };

            let name = if self.verbose {
                format!("{}\n> {}", assignment.name, self.truncate(&assignment.description, 100))
            } else {
                assignment.name.clone()
            };

            println!(
                "| {} | {} | {} | {} | {} |",
                assignment.id, name, due_date, points, status
            );
        }

        Ok(())
    }

    fn output_csv(&self, assignments: &[Assignment]) -> Result<(), anyhow::Error> {
        if assignments.is_empty() {
            return Ok(());
        }

        // Header
        println!(
            "id,name,due_at,points_possible,workflow_state,submission_status,score,grade"
        );

        // Rows
        for assignment in assignments {
            let due_at = assignment
                .due_at
                .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_else(|| "".to_string());

            let points = assignment
                .points_possible
                .map(|p| p.to_string())
                .unwrap_or_else(|| "".to_string());

            let (submission_status, score, grade) = match &assignment.submission {
                Some(sub) => (
                    sub.workflow_state.clone(),
                    sub.score
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "".to_string()),
                    sub.grade.clone().unwrap_or_else(|| "".to_string()),
                ),
                None => ("not_submitted".to_string(), "".to_string(), "".to_string()),
            };

            // Escape commas and quotes in name
            let name = format!("\"{}\"", assignment.name.replace('"', "\"\""));

            println!(
                "{},{},{},{},{},{},{},{}",
                assignment.id, name, due_at, points, assignment.workflow_state, submission_status, score, grade
            );
        }

        Ok(())
    }

    fn output_table(&self, assignments: &[Assignment]) -> Result<(), anyhow::Error> {
        if assignments.is_empty() {
            println!("{}", "No assignments found.".bright_yellow());
            return Ok(());
        }

        println!(
            "{}",
            format!("Found {} assignment(s)", assignments.len()).bright_green()
        );
        println!();

        for assignment in assignments {
            // Assignment name and ID
            println!(
                "{} {}",
                "Assignment:".bright_cyan(),
                assignment.name.bold()
            );
            println!("{} {}", "ID:".cyan(), assignment.id.to_string());

            // Due date
            if let Some(due_at) = assignment.due_at {
                let now = Utc::now();
                let due_str = due_at.format("%B %d, %Y at %I:%M %p").to_string();

                let status = if due_at < now {
                    "Past due".red()
                } else {
                    "Upcoming".green()
                };

                println!("{} {} ({})", "Due:".cyan(), due_str, status);
            } else {
                println!("{} {}", "Due:".cyan(), "No due date".bright_yellow());
            }

            // Points
            if let Some(points) = assignment.points_possible {
                println!("{} {}", "Points:".cyan(), format!("{}", points));
            }

            // Submission status
            match &assignment.submission {
                Some(sub) => {
                    let status_icon = match sub.workflow_state.as_str() {
                        "submitted" => "✓",
                        "graded" => "✓",
                        "missing" => "❌",
                        "late" => "⏰",
                        _ => "○",
                    };

                    let status_text = match sub.workflow_state.as_str() {
                        "graded" => {
                            if let Some(grade) = &sub.grade {
                                format!("Graded: {}", grade)
                            } else if let Some(score) = sub.score {
                                format!("Graded: {}", score)
                            } else {
                                "Graded".to_string()
                            }
                        }
                        "submitted" => "Submitted".to_string(),
                        "missing" => "Missing".to_string(),
                        "late" => "Late".to_string(),
                        other => format!("{}", other),
                    };

                    println!(
                        "{} {} {}",
                        "Status:".cyan(),
                        status_icon,
                        status_text
                    );
                }
                None => {
                    println!(
                        "{} {} {}",
                        "Status:".cyan(),
                        "📝",
                        "Not submitted".bright_yellow()
                    );
                }
            }

            // Submission type
            if !assignment.submission_types.is_empty() {
                println!(
                    "{} {}",
                    "Type:".cyan(),
                    assignment
                        .submission_types
                        .join(", ")
                        .replace("online_", "")
                        .replace("_", " ")
                );
            }

            // Description (verbose mode)
            if self.verbose && !assignment.description.is_empty() {
                let description_plain = html2md::parse_html(&assignment.description);
                println!(
                    "\n{} {}",
                    "Description:".cyan(),
                    self.truncate(&description_plain, 500)
                );
            }

            // URL
            if !assignment.html_url.is_empty() {
                println!("{} {}", "URL:".cyan(), assignment.html_url);
            }

            println!("{}", "─".repeat(60));
        }

        Ok(())
    }

    fn truncate(&self, s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }
}

impl Assignment {
    /// Fetch all assignments for a course
    pub async fn fetch_all(
        course_id: u32,
        base_url: &str,
        client: &reqwest::Client,
    ) -> Result<Vec<Self>, anyhow::Error> {
        let response = client
            .get(format!(
                "{}/api/v1/courses/{}/assignments?per_page=100&include[]=submissions",
                base_url, course_id
            ))
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch assignments: {} {}",
                response.status(),
                response.text().await?
            );
        }

        let assignments: Vec<Assignment> = response.json().await?;
        log::info!("Fetched {} assignments for course {}", assignments.len(), course_id);
        Ok(assignments)
    }

    /// Fetch a single assignment by ID
    pub async fn fetch(
        course_id: u32,
        assignment_id: u32,
        base_url: &str,
        client: &reqwest::Client,
    ) -> Result<Self, anyhow::Error> {
        let response = client
            .get(format!(
                "{}/api/v1/courses/{}/assignments/{}?include[]=submissions",
                base_url, course_id, assignment_id
            ))
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch assignment: {} {}",
                response.status(),
                response.text().await?
            );
        }

        let assignment: Assignment = response.json().await?;
        log::info!("Fetched assignment {} for course {}", assignment_id, course_id);
        Ok(assignment)
    }
}
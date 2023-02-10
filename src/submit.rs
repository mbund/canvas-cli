#![allow(dead_code)]

use crate::{submit::query_assignments::SubmissionType, Config};
use fuzzy_matcher::FuzzyMatcher;
use graphql_client::GraphQLQuery;
use indicatif::ProgressStyle;
use inquire::*;

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "generated/schema.json",
    query_path = "src/query_assignments.graphql",
    response_derives = "Debug, Clone"
)]
pub struct QueryAssignments;

#[derive(Debug)]
struct Course {
    name: String,
    id: String,
}

#[derive(Debug)]
struct Assignment {
    name: String,
    id: String,
    due_at: Option<DateTime>,
    course: Course,
    submitted: bool,
    submission_type: SubmissionType,
}

#[derive(clap::Parser, Debug)]
/// Submit Canvas assignment
pub struct SubmitCommand {
    /// Name of assignment to submit to
    assignment_name: String,

    /// File(s)
    files: Vec<String>,
}

impl SubmitCommand {
    pub async fn action(&self, cfg: &Config) -> Result<(), anyhow::Error> {
        // let client = reqwest::Client::builder()
        //     .default_headers(
        //         std::iter::once((
        //             reqwest::header::AUTHORIZATION,
        //             reqwest::header::HeaderValue::from_str(&format!("Bearer {}", cfg.access_token))
        //                 .unwrap(),
        //         ))
        //         .collect(),
        //     )
        //     .build()?;

        // find assignment

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("Querying assignment information");
        // for _ in 0..40 {
        //     spinner.inc(1);
        //     std::thread::sleep(std::time::Duration::from_millis(100));
        // }

        // let spinner_task = tokio::task::spawn(async move {
        //     loop {
        //         spinner.inc(1);
        //         tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        //     }
        // });

        let url = cfg.url.to_owned();
        let access_token = cfg.access_token.to_owned();
        let task = tokio::task::spawn(async move {
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

            graphql_client::reqwest::post_graphql::<QueryAssignments, _>(
                &client,
                url + "api/graphql",
                query_assignments::Variables {},
            )
            .await
            .unwrap()
            .data
            .unwrap()
        });

        loop {
            spinner.inc(1);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if task.is_finished() {
                break;
            }
        }

        let queried_assignments = task.await?;

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

                            let submission_type = assignment_props
                                .submission_types
                                .as_ref()
                                .unwrap()
                                .into_iter()
                                .next()
                                .unwrap();

                            Assignment {
                                name: assignment_props.name.as_ref().unwrap().to_owned(),
                                id: assignment_props.id.to_owned(),
                                due_at: assignment_props.due_at.to_owned(),
                                submitted,
                                submission_type: submission_type.to_owned(),
                                course: Course {
                                    name: course.name.to_owned(),
                                    id: course.id.to_owned(),
                                },
                            }
                        })
                })
            })
            .collect();

        // get course names
        let mut courses: Vec<String> = queried_assignments
            .all_courses
            .iter()
            .flat_map(|all_courses| all_courses.iter().map(|course| course.name.to_owned()))
            .collect();
        courses.sort_by(|a, b| a.cmp(b));

        // prompt user to select course from list
        let course = Select::new("Course?", courses).prompt()?;

        // filter assignments to a nice list
        assignments.retain(|assignment| match assignment.submission_type {
            SubmissionType::online_upload => true,
            _ => false, // TODO: support more upload types
        });
        assignments.retain(|assignment| assignment.course.name == course);
        assignments.sort_by(|a, b| {
            let submit_order = a.submitted.cmp(&b.submitted);
            if submit_order == std::cmp::Ordering::Equal {
                if a.due_at.is_none() {
                    return std::cmp::Ordering::Greater;
                } else if b.due_at.is_none() {
                    return std::cmp::Ordering::Less;
                } else {
                    return a.due_at.unwrap().cmp(&b.due_at.unwrap());
                }
            } else {
                return submit_order;
            }
        });

        // use the assignments to make a list to prompt the user with
        let options: Vec<String> = assignments
            .into_iter()
            .map(|assignment| {
                format!(
                    "{} {}",
                    assignment.name,
                    if assignment.submitted { "✓" } else { " " },
                )
            })
            .collect();

        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
        let _assignment = Select::new("Assignment?", options)
            .with_filter(&|input, _, string_value, _| {
                matcher.fuzzy_match(string_value, input).is_some()
            })
            .prompt()?;

        Ok(())
    }
}

#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::{submit::query_assignments::SubmissionType, Config};
use colored::Colorize;
use fuzzy_matcher::FuzzyMatcher;
use graphql_client::GraphQLQuery;
use indicatif::ProgressStyle;
use inquire::*;
use serde::Deserialize;

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
    submission_type: SubmissionType,
}

impl Display for Assignment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.name, if self.submitted { " ✓" } else { "" })
    }
}

#[derive(clap::Parser, Debug)]
/// Submit Canvas assignment
pub struct SubmitCommand {
    /// Name of assignment to submit to
    assignment_name: String,

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

impl SubmitCommand {
    pub async fn action(&self, cfg: &Config) -> Result<(), anyhow::Error> {
        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("Querying assignment information");

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

        let spinner_clone = spinner.clone();
        let spinner_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                spinner_clone.inc(1);
            }
        });

        let favorites_request = client
            .get(format!("{}api/v1/users/self/favorites/courses", url))
            .send();

        let colors_request = client
            .get(format!("{}api/v1/users/self/colors", url))
            .send();

        let colors = colors_request.await?.json::<ColorsResponse>().await?;
        let favorites = favorites_request
            .await?
            .json::<Vec<FavoritesResponse>>()
            .await?;
        let queried_assignments = graphql_assignment_request.await?.data.unwrap();
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
        assignments.retain(|assignment| match assignment.submission_type {
            SubmissionType::online_upload => true,
            _ => false, // TODO: support more upload types
        });
        assignments.retain(|assignment| assignment.course == course);
        assignments.sort_by(|a, b| a.submitted.cmp(&b.submitted).then(a.due_at.cmp(&b.due_at)));
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
        let _assignment = Select::new("Assignment?", assignments)
            .with_filter(&|input, _, string_value, _| {
                matcher.fuzzy_match(string_value, input).is_some()
            })
            .prompt()?;

        Ok(())
    }
}

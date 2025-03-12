use colored::Colorize;
use inquire::Select;
use reqwest::Client;
use serde_derive::Deserialize;
use std::{collections::HashMap, fmt::Display};

pub type DateTime = chrono::DateTime<chrono::Utc>;

#[derive(Debug, Hash, Clone, PartialEq, Eq)]
pub struct Course {
    pub name: String,
    pub id: u32,
    is_favorite: bool,
    css_color: Option<String>,
    created_at: DateTime,
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
struct ColorsResponse {
    custom_colors: HashMap<String, String>,
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
            "█ ".truecolor(color[0], color[1], color[2]),
            self.name,
            if self.is_favorite { " ★" } else { "" }.yellow()
        )
    }
}

impl Course {
    pub async fn fetch(
        course_id: Option<u32>,
        base_url: &str,
        client: &Client,
    ) -> Result<Course, anyhow::Error> {
        Ok(if let Some(course_id) = course_id {
            let course_response = client
                .get(format!(
                    "{}/api/v1/courses/{}?include[]=favorites&include[]=concluded",
                    base_url, course_id
                ))
                .send()
                .await?
                .json::<CourseResponse>()
                .await?;
            log::info!("Made REST request to get course information");

            let course_colors: HashMap<u32, String> = client
                .get(format!("{}/api/v1/users/self/colors", base_url))
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
                    base_url
                ))
                .send()
                .await?
                .json::<Vec<serde_json::Value>>()
                .await?
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect::<Vec<CourseResponse>>();

            log::info!("Made REST request to get favorite courses");

            let course_colors: HashMap<u32, String> = client
                .get(format!("{}/api/v1/users/self/colors", base_url))
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
        })
    }
}

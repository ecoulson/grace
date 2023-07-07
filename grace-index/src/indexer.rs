extern crate rocket;

use crate::database;
use rocket::serde::Serialize;
use rocket::serde::{json::Json, Deserialize};
use mysql::prelude::Queryable;
use std::cmp::min;

#[derive(Debug)]
pub struct Indexer {
    pub id: u64,
    pub url_queue: Vec<Link>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    pub id: u64,
    pub url: String,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Query {
    line: String,
}

#[rocket::post("/", data = "<query>")]
pub fn route<'a>(query: Json<Query>) -> Json<Vec<Link>> {
    let mut connection = database::connect_to_database().expect("Failed to connect to PlanetScale");
    let links = connection
        .query_map("SELECT id, url, text from links", |(id, url, text)| Link {
            id,
            url,
            text: Some(text),
        })
        .expect("Failed to retrieve links");

    let good_links = links
        .iter()
        .filter(|link| is_good_link(query.line.trim().split(".").last().unwrap(), link))
        .map(|link| Link {
            url: link.url.clone(),
            id: link.id.clone(),
            text: None,
        })
        .collect();

    Json(good_links)
}

fn is_good_link(query: &str, link: &Link) -> bool {
    if let Some(text) = &link.text {
        for line in text.split("\n") {
            let query: Vec<char> = query.chars().collect();
            let line: Vec<char> = line.trim().chars().collect();
            let distance = get_distance(&query, &line);
            let match_score = distance as f64 / query.len() as f64;

            let x = line.iter().collect::<String>();
            let y = query.iter().collect::<String>();
            dbg!(match_score, x, y);
            println!();

            if match_score < 0.25 {
                return true;
            }
        }
    }

    return false;
}

fn get_distance(query: &[char], line: &[char]) -> usize {
    let query_char_count = query.len();
    let line_char_count = line.len();

    if line_char_count < query_char_count {
        return query_char_count;
    }

    let checks = line_char_count - query_char_count + 1;
    let mut min_distance = usize::MAX;

    for i in 0..checks {
        min_distance = min(
            min_distance,
            levenshtein(query, &line[i..i + query_char_count]),
        );
    }

    return min_distance;
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut table = vec![vec![0; b.len() + 1]; a.len() + 1];

    for i in 0..a.len() + 1 {
        table[i][0] = i;
    }

    for j in 0..b.len() + 1 {
        table[0][j] = j;
    }

    for i in 1..a.len() + 1 {
        for j in 1..b.len() + 1 {
            if a[i - 1] == b[j - 1] {
                table[i][j] = table[i - 1][j - 1];
            } else {
                let insertion = 1 + table[i][j - 1];
                let deletion = 1 + table[i - 1][j];
                let replacement = 1 + table[i - 1][j - 1];

                table[i][j] = min(min(insertion, deletion), replacement);
            }
        }
    }

    return table[a.len()][b.len()];
}

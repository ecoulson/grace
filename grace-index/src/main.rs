#[macro_use]
extern crate rocket;
extern crate dotenv;

use dotenv::dotenv;
use mysql::{params, Transaction, TxOpts};
use mysql::{prelude::Queryable, PooledConn};
use rocket::serde::{json::Json, Deserialize, Serialize};
use std::cmp::min;
use std::env;
use std::str::Chars;
use std::time::Duration;
use ureq::AgentBuilder;

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Link {
    id: u64,
    url: String,
    text: Option<String>,
}

fn connect_to_database() -> Result<mysql::PooledConn, mysql::Error> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL not found");
    let builder = mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&url).unwrap());
    let pool = mysql::Pool::new(builder.ssl_opts(mysql::SslOpts::default())).unwrap();
    pool.get_conn()
}

#[derive(Debug, Deserialize)]
struct Query {
    line: String,
}

#[post("/", data = "<query>")]
fn index<'a>(query: Json<Query>) -> Json<Vec<Link>> {
    let mut connection = connect_to_database().expect("Failed to connect to PlanetScale");
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

#[derive(Debug)]
struct Indexer {
    id: u64,
    url_queue: Vec<Link>,
}

pub enum GraceError {
    MySQL(mysql::Error),
    UReq(ureq::Error),
    IO(std::io::Error),
}

impl From<mysql::Error> for GraceError {
    fn from(value: mysql::Error) -> Self {
        Self::MySQL(value)
    }
}

impl From<ureq::Error> for GraceError {
    fn from(value: ureq::Error) -> Self {
        Self::UReq(value)
    }
}

impl From<std::io::Error> for GraceError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

fn crawl() -> Result<(), GraceError> {
    let mut connection = connect_to_database().expect("Failed to connect to PlanetScale");
    let mut indexers = load_indexers(&mut connection)?;

    if indexers.is_empty() {
        indexers.push(create_default_indexer(&mut connection)?);
    }

    let indexer = indexers
        .first_mut()
        .expect("At least one indexer should exist");

    while let Some(link) = indexer.url_queue.pop() {
        let agent = AgentBuilder::new()
            .timeout_read(Duration::from_secs(5))
            .build();
        let html = agent.get(link.url.as_str()).call()?.into_string()?;
        let text = parse(html);

        let mut transaction = connection.start_transaction(TxOpts::default())?;
        transaction.exec_drop(
            "UPDATE links SET text = :text, indexed_at = now() WHERE url = :url",
            params! {
                text,
                "url" => link.url,
            },
        )?;
        transaction.exec_drop(
            "DELETE FROM queue_links WHERE link_id = :link_id",
            params! {
                "link_id" => link.id
            },
        )?;
        transaction.commit()?;

        println!("Completed update");
    }

    Ok(())
}

fn parse(html: String) -> String {
    let mut in_tag = false;
    let mut text = String::new();

    for char in html.chars() {
        match char {
            '<' => in_tag = true,
            '>' => in_tag = false,
            ch => {
                if !in_tag {
                    text.push(ch)
                }
            }
        }
    }

    text
}

fn load_indexers(connection: &mut PooledConn) -> Result<Vec<Indexer>, mysql::Error> {
    let mut transaction = connection.start_transaction(TxOpts::default())?;

    let ids: Vec<u64> = transaction.query("SELECT id FROM indexers")?;
    let indexers = ids
        .iter()
        .map(|id| Indexer {
            id: *id,
            url_queue: load_indexer_queue(&mut transaction, *id)
                .expect("Failed to load indexer queue"),
        })
        .collect();

    transaction.commit()?;

    Ok(indexers)
}

fn create_default_indexer(connection: &mut PooledConn) -> Result<Indexer, mysql::Error> {
    let mut transaction = connection.start_transaction(TxOpts::default())?;

    transaction.query_drop("INSERT INTO indexers (id) VALUES (NULL)")?;
    let id = transaction
        .last_insert_id()
        .expect("Transaction should have commited an indexer");
    let default_url = String::from("https://neovim.io/doc/user/api.html#api");
    transaction.exec_drop(
        "INSERT INTO links (id, url) VALUES (NULL, :url)",
        params! {
            "url" => &default_url
        },
    )?;
    let link_id = transaction
        .last_insert_id()
        .expect("Transaction should have commited a link");
    transaction.exec_drop(
        "INSERT INTO queue_links (id, link_id) VALUES (:id, :link_id)",
        params! {
            id, link_id
        },
    )?;
    transaction.commit()?;

    Ok(Indexer {
        id,
        url_queue: vec![Link {
            text: None,
            url: default_url,
            id: link_id,
        }],
    })
}

fn load_indexer_queue(
    transaction: &mut Transaction,
    indexer_id: u64,
) -> Result<Vec<Link>, mysql::Error> {
    let link_ids: Vec<u64> = transaction.exec(
        "SELECT link_id FROM queue_links WHERE id = :indexer_id",
        params! {
            indexer_id
        },
    )?;
    let urls = transaction.exec_map(
        "SELECT id, url FROM links WHERE id IN (:link_ids)",
        params! {
            "link_ids" => link_ids.iter()
                                  .map(|x| x.to_string())
                                  .collect::<Vec<String>>()
                                  .join(", "),
        },
        |(id, url)| Link {
            id,
            url,
            text: None,
        },
    )?;

    Ok(urls)
}

#[launch]
async fn rocket() -> _ {
    dotenv().ok();

    rocket::tokio::spawn(async { crawl() });
    rocket::build().mount("/", routes![index])
}

#[macro_use]
extern crate rocket;
extern crate dotenv;

use dotenv::dotenv;
use rocket::serde::Serialize;
use grace::{crawler, indexer};

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Link {
    id: u64,
    url: String,
    text: Option<String>,
}

#[launch]
async fn rocket() -> _ {
    dotenv().ok();

    rocket::tokio::spawn(async { crawler::crawl() });
    rocket::build().mount("/", routes![indexer::route])
}

#[macro_use]
extern crate rocket;
extern crate dotenv;

use dotenv::dotenv;
use mysql::prelude::Queryable;
use rocket::serde::{json::Json, Serialize};
use std::env;

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Link {
    id: u64,
    url: String,
}

fn connect_to_database() -> Result<mysql::PooledConn, mysql::Error> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL not found");
    let builder = mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&url).unwrap());
    let pool = mysql::Pool::new(builder.ssl_opts(mysql::SslOpts::default())).unwrap();
    pool.get_conn()
}

#[get("/")]
fn index<'a>() -> Json<Vec<Link>> {
    let mut connection = connect_to_database().expect("Failed to connect to PlanetScale");
    let links = connection
        .query_map("SELECT id, url from links", |(id, url)| Link { id, url })
        .expect("Failed to retrieve links");

    Json(links)
}

#[launch]
fn rocket() -> _ {
    dotenv().ok();

    rocket::build().mount("/", routes![index])
}

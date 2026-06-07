use crate::{
    api::auth,
    core::retrieve::{self},
};

mod api;
mod core;
mod utils;

fn main() {
    auth::LoginBuilder::new()
        .identity("Aurzex")
        .password("CODExhr1106.mao")
        .execute();

    let comments = retrieve::DataQuery::new()
        .query_comments()
        .source(retrieve::CommentSource::Work)
        .limit(Some(50))
        .mode(retrieve::CommentQueryMode::Comments)
        .target_id(130866720)
        .execute()
        .unwrap();
    dbg!(comments);
}

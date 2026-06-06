use crate::{
    api::auth,
    core::retrieve::{self},
};

mod api;
mod core;
mod utils;

fn main() {
    auth::LoginBuilder::new()
        .identity("identity")
        .password("password")
        .role(auth::UserRole::Admin)
        .method(auth::LoginMethod::AdminPassword)
        .execute();

    let comments = retrieve::DataQuery::new()
        .query_comments()
        .source(retrieve::CommentSource::Work)
        .limit(Some(50))
        .mode(retrieve::CommentQueryMode::Comments)
        .target_id(130866720)
        .execute();
    dbg!(comments);

    let result = retrieve::DataQuery::new().compute_admin_report_stats();
    dbg!(result);
}

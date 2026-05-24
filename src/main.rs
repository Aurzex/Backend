use crate::{api::auth, core::executer::ReportProcessor};

mod api;
mod core;
mod utils;
use tokio::main;
#[tokio::main]
async fn main() {
    auth::LoginBuilder::new()
        .identity("fengji03")
        .password("CODExhr1106.mao")
        .role(auth::UserRole::Admin)
        .method(auth::LoginMethod::AdminPassword)
        .execute()
        .await
        .unwrap();

    let processor = ReportProcessor::new().await;
    processor.process_all_reports(223).await.unwrap();
}

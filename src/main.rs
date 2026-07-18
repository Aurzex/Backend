use crate::{api::auth, core::services};

mod api;
mod core;
mod utils;

fn main() {
    let result = auth::LoginBuilder::new()
        .identity("")
        .password("")
        .method(auth::LoginMethod::PasswordV1)
        .execute();
    println!("{}", result.unwrap().token);
}

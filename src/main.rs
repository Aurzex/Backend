use crate::{api::auth, core::services};

mod api;
mod core;
mod utils;



fn main() {
    let result = auth::LoginBuilder::new()
        .identity("Aurzex")
        .password("CODExhr1106.mao")
        .method(auth::LoginMethod::PasswordV1)
        .execute();
    println!("{}", result.unwrap().token);
}

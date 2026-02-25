mod api;
mod utils;
fn main() {
    // Make auth mutable
    let mut auth = api::auth::AuthManager::new();
    let _au = auth
        .login(
            Some("Aurzex"),
            Some("CODExhr1106.mao"),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .token;
    print!("OK{}", _au)
}

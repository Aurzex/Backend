use pyo3::prelude::*;
mod api;
mod utils;

/// A Python module implemented in Rust.
#[pymodule]
mod backend {
    use pyo3::prelude::*;

    use crate::api;

    /// Formats the sum of two numbers as string.
    #[pyfunction]
    fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
        Ok((a + b).to_string())
    }
    #[pyfunction]
    fn rs_test() {
        // Make auth mutable
        let mut auth = api::auth::AuthManager::new();
        let _au = auth.login(
            Some("Aurzex"),
            Some("CODExhr1106.mao"),
            None,
            None,
            None,
            None,
            None,
        );
    }
}

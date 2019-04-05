#[derive(Debug)]
pub struct AppError {
    kind: String,
    message: String,
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError {
            kind: String::from("io"),
            message: error.to_string(),
        }
    }
}

impl From<std::str::Utf8Error> for AppError {
    fn from(error: std::str::Utf8Error) -> Self {
        AppError {
            kind: String::from("UTF-8"),
            message: error.to_string(),
        }
    }
}

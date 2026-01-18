use std::fmt;

#[derive(Debug, Clone)]
pub enum ServiceError {
    InvalidInput(String),
    NotFound(String),
    Internal(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceError::InvalidInput(message) => write!(f, "{}", message),
            ServiceError::NotFound(message) => write!(f, "{}", message),
            ServiceError::Internal(message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for ServiceError {}

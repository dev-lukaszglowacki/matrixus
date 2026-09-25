//! Error definitions for matrix-core

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MatrixError {
    #[error("Matrix SDK error: {0}")]
    Sdk(#[from] matrix_sdk::Error),

    #[error("Client build error: {0}")]
    ClientBuild(#[from] matrix_sdk::ClientBuildError),

    #[error("URL parsing error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Room not found: {0}")]
    RoomNotFound(String),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MatrixError>;

use std::path::PathBuf;

use thiserror::Error;

/// Core error type for launcher operations.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Failed to determine standard application directories")]
    ProjectDirsUnavailable,

    #[error("I/O error at path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse configuration file: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Failed to serialize configuration: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("Failed to find any Java runtime")]
    JavaNotFound,

    #[error("Failed to probe Java version: {path}: {reason}")]
    JavaProbeFailed { path: PathBuf, reason: String },

    #[error("Java version mismatch: required {required_major}, found {found_major}, path: {path}")]
    JavaVersionMismatch {
        required_major: u32,
        found_major: u32,
        path: PathBuf,
    },

    #[error("Instance '{id}' not found")]
    InstanceNotFound { id: String },

    #[error("Instance '{id}' already exists")]
    InstanceAlreadyExists { id: String },

    #[error("Invalid instance id '{id}': {reason}")]
    InvalidInstanceId { id: String, reason: String },

    #[error("System clock error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Invalid memory configuration: min {min_mb} MB cannot exceed max {max_mb} MB")]
    InvalidMemoryConfig { min_mb: u32, max_mb: u32 },

    #[error("Failed to spawn game process with Java '{java_path}': {source}")]
    ProcessSpawn {
        java_path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to join classpath entries: {0}")]
    JoinPaths(#[from] std::env::JoinPathsError),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_error_display() {
        let err = CoreError::ProjectDirsUnavailable;
        assert_eq!(
            err.to_string(),
            "Failed to determine standard application directories"
        );

        let io_err = CoreError::Io {
            path: PathBuf::from("/test/path"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        assert!(io_err.to_string().contains("/test/path"));
        assert!(io_err.to_string().contains("file not found"));
    }

    #[test]
    fn test_java_error_display() {
        let not_found = CoreError::JavaNotFound;
        assert_eq!(not_found.to_string(), "Failed to find any Java runtime");

        let probe_failed = CoreError::JavaProbeFailed {
            path: PathBuf::from("/mock/bin/java"),
            reason: "Missing java.version property".into(),
        };
        assert_eq!(
            probe_failed.to_string(),
            "Failed to probe Java version: /mock/bin/java: Missing java.version property"
        );

        let mismatch = CoreError::JavaVersionMismatch {
            required_major: 17,
            found_major: 8,
            path: PathBuf::from("/mock/jvm/java-8/bin/java"),
        };
        assert_eq!(
            mismatch.to_string(),
            "Java version mismatch: required 17, found 8, path: /mock/jvm/java-8/bin/java"
        );
    }

    #[test]
    fn test_instance_error_display() {
        let not_found = CoreError::InstanceNotFound {
            id: "inst1".to_string(),
        };
        assert_eq!(not_found.to_string(), "Instance 'inst1' not found");

        let already_exists = CoreError::InstanceAlreadyExists {
            id: "inst1".to_string(),
        };
        assert_eq!(
            already_exists.to_string(),
            "Instance 'inst1' already exists"
        );

        let invalid_id = CoreError::InvalidInstanceId {
            id: "bad/id".to_string(),
            reason: "contains slash".to_string(),
        };
        assert_eq!(
            invalid_id.to_string(),
            "Invalid instance id 'bad/id': contains slash"
        );

        let sys_time_err = std::time::UNIX_EPOCH
            .duration_since(std::time::SystemTime::now())
            .expect_err("system time should be after epoch");
        let sys_err = CoreError::SystemTime(sys_time_err);
        assert!(sys_err.to_string().starts_with("System clock error:"));
    }
}

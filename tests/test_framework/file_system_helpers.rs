//! File system helpers for test environments
//!
//! Provides utilities for file operations, directory management, and
//! test data setup in isolated test environments.

use anyhow::{Context, Result};
use sha2::Digest;
use std::fs;
use std::path::{Path, PathBuf};

/// File system helpers for test operations
///
/// Provides convenient methods for file and directory operations in test environments
/// with proper error handling and context.
#[derive(Debug)]
pub struct FileSystemHelpers {
    base_dir: PathBuf,
}

impl FileSystemHelpers {
    /// Create new file system helpers for the given base directory
    pub fn new(base_dir: &Path) -> Self {
        FileSystemHelpers {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Get the base directory path
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolve a path relative to the base directory
    pub fn resolve_path(&self, relative_path: &str) -> PathBuf {
        self.base_dir.join(relative_path)
    }

    /// Write content to a file (creates parent directories if needed)
    pub fn write_file(&self, relative_path: &str, content: &str) -> Result<()> {
        let full_path = self.resolve_path(relative_path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directory: {:?}", parent))?;
        }

        fs::write(&full_path, content)
            .with_context(|| format!("Failed to write file: {:?}", full_path))?;

        Ok(())
    }

    /// Read content from a file
    pub fn read_file(&self, relative_path: &str) -> Result<String> {
        let full_path = self.resolve_path(relative_path);

        let content = fs::read_to_string(&full_path)
            .with_context(|| format!("Failed to read file: {:?}", full_path))?;

        Ok(content)
    }

    /// Check if a file exists
    pub fn file_exists(&self, relative_path: &str) -> bool {
        let full_path = self.resolve_path(relative_path);
        full_path.exists() && full_path.is_file()
    }

    /// Check if a directory exists
    pub fn dir_exists(&self, relative_path: &str) -> bool {
        let full_path = self.resolve_path(relative_path);
        full_path.exists() && full_path.is_dir()
    }

    /// Create a directory (including parent directories)
    pub fn create_dir(&self, relative_path: &str) -> Result<()> {
        let full_path = self.resolve_path(relative_path);

        fs::create_dir_all(&full_path)
            .with_context(|| format!("Failed to create directory: {:?}", full_path))?;

        Ok(())
    }

    /// Create a directory structure from a map of paths to content
    pub fn create_structure(
        &self,
        structure: &std::collections::HashMap<&str, Option<&str>>,
    ) -> Result<()> {
        for (path, content) in structure {
            if let Some(content) = content {
                // It's a file
                self.write_file(path, content)?;
            } else {
                // It's a directory
                self.create_dir(path)?;
            }
        }
        Ok(())
    }

    /// Remove a file or directory (recursively)
    pub fn remove(&self, relative_path: &str) -> Result<()> {
        let full_path = self.resolve_path(relative_path);

        if full_path.exists() {
            if full_path.is_dir() {
                fs::remove_dir_all(&full_path)
                    .with_context(|| format!("Failed to remove directory: {:?}", full_path))?;
            } else {
                fs::remove_file(&full_path)
                    .with_context(|| format!("Failed to remove file: {:?}", full_path))?;
            }
        }

        Ok(())
    }

    /// Copy a file from one location to another
    pub fn copy_file(&self, src_relative: &str, dst_relative: &str) -> Result<()> {
        let src_path = self.resolve_path(src_relative);
        let dst_path = self.resolve_path(dst_relative);

        // Create parent directories for destination
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directory: {:?}", parent))?;
        }

        fs::copy(&src_path, &dst_path).with_context(|| {
            format!("Failed to copy file from {:?} to {:?}", src_path, dst_path)
        })?;

        Ok(())
    }

    /// Move/rename a file or directory
    pub fn move_path(&self, src_relative: &str, dst_relative: &str) -> Result<()> {
        let src_path = self.resolve_path(src_relative);
        let dst_path = self.resolve_path(dst_relative);

        // Create parent directories for destination
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directory: {:?}", parent))?;
        }

        fs::rename(&src_path, &dst_path).with_context(|| {
            format!("Failed to move path from {:?} to {:?}", src_path, dst_path)
        })?;

        Ok(())
    }

    /// Append content to a file (creates file if it doesn't exist)
    pub fn append_file(&self, relative_path: &str, content: &str) -> Result<()> {
        let full_path = self.resolve_path(relative_path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directory: {:?}", parent))?;
        }

        fs::write(&full_path, content)
            .with_context(|| format!("Failed to append to file: {:?}", full_path))?;

        Ok(())
    }

    /// Get file size in bytes
    pub fn file_size(&self, relative_path: &str) -> Result<u64> {
        let full_path = self.resolve_path(relative_path);

        let metadata = fs::metadata(&full_path)
            .with_context(|| format!("Failed to get file metadata: {:?}", full_path))?;

        Ok(metadata.len())
    }

    /// List files in a directory (non-recursive)
    pub fn list_dir(&self, relative_path: &str) -> Result<Vec<String>> {
        let full_path = self.resolve_path(relative_path);

        let entries = fs::read_dir(&full_path)
            .with_context(|| format!("Failed to read directory: {:?}", full_path))?;

        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.with_context(|| "Failed to read directory entry")?;
            let name = entry.file_name().to_string_lossy().to_string();
            files.push(name);
        }

        files.sort();
        Ok(files)
    }

    /// Create a temporary file with unique name
    pub fn temp_file(&self, prefix: &str, suffix: &str) -> Result<PathBuf> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let filename = format!("{}_{}{}", prefix, timestamp, suffix);
        let full_path = self.resolve_path(&filename);

        // Create an empty file
        fs::File::create(&full_path)
            .with_context(|| format!("Failed to create temp file: {:?}", full_path))?;

        Ok(full_path)
    }

    /// Create a JSON file from a serializable value
    pub fn write_json<T: serde::Serialize>(&self, relative_path: &str, data: &T) -> Result<()> {
        let json_content = serde_json::to_string_pretty(data)
            .with_context(|| "Failed to serialize data to JSON")?;

        self.write_file(relative_path, &json_content)
    }

    /// Read and parse a JSON file
    pub fn read_json<T: for<'de> serde::Deserialize<'de>>(&self, relative_path: &str) -> Result<T> {
        let content = self.read_file(relative_path)?;

        let data: T = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON from file: {}", relative_path))?;

        Ok(data)
    }

    /// Calculate SHA256 hash of a file
    pub fn file_hash(&self, relative_path: &str) -> Result<String> {
        use std::io::Read;

        let full_path = self.resolve_path(relative_path);
        let mut file = fs::File::open(&full_path)
            .with_context(|| format!("Failed to open file: {:?}", full_path))?;

        let mut hasher = sha2::Sha256::new();
        let mut buffer = [0; 8192];

        loop {
            let bytes_read = file
                .read(&mut buffer)
                .with_context(|| "Failed to read from file")?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_file_operations() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());

        // Test write and read
        fs.write_file("test.txt", "Hello, World!")?;
        assert_eq!(fs.read_file("test.txt")?, "Hello, World!");
        assert!(fs.file_exists("test.txt"));

        // Test directory creation
        fs.create_dir("subdir")?;
        assert!(fs.dir_exists("subdir"));

        // Test file in subdirectory
        fs.write_file("subdir/nested.txt", "Nested content")?;
        assert_eq!(fs.read_file("subdir/nested.txt")?, "Nested content");

        // Test structure creation
        let mut structure = HashMap::new();
        structure.insert("project", None);
        structure.insert("project/src", None);
        structure.insert("project/src/main.rs", Some("fn main() {}"));
        structure.insert("project/Cargo.toml", Some("[package]\nname = \"test\""));

        fs.create_structure(&structure)?;
        assert!(fs.dir_exists("project"));
        assert!(fs.file_exists("project/src/main.rs"));
        assert_eq!(fs.read_file("project/src/main.rs")?, "fn main() {}");

        Ok(())
    }

    #[test]
    fn test_json_operations() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let fs = FileSystemHelpers::new(temp_dir.path());

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestStruct {
            name: String,
            value: i32,
        }

        let data = TestStruct {
            name: "test".to_string(),
            value: 42,
        };

        fs.write_json("test.json", &data)?;
        let loaded: TestStruct = fs.read_json("test.json")?;

        assert_eq!(data, loaded);

        Ok(())
    }
}

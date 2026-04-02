use std::io;
use codex_utils_absolute_path::AbsolutePathBuf;
use crate::{CopyOptions, CreateDirectoryOptions, ExecutorFileSystem, FileMetadata, FileSystemResult, ReadDirectoryEntry, RemoveOptions};

pub struct WasiFs;

#[async_trait::async_trait]
impl ExecutorFileSystem for WasiFs {
    async fn read_file(&self, path: &AbsolutePathBuf) -> FileSystemResult<Vec<u8>> {
        std::fs::read(path)
    }

    async fn write_file(&self, path: &AbsolutePathBuf, contents: Vec<u8>) -> FileSystemResult<()> {
        std::fs::write(path, contents)
    }

    async fn create_directory(&self, path: &AbsolutePathBuf, options: CreateDirectoryOptions) -> FileSystemResult<()> {
        if options.recursive {
            std::fs::create_dir_all(path)
        } else {
            std::fs::create_dir(path)
        }
    }

    async fn get_metadata(&self, path: &AbsolutePathBuf) -> FileSystemResult<FileMetadata> {
        let meta = std::fs::metadata(path)?;
        let is_directory = meta.is_dir();
        let is_file = meta.is_file();
        let created_at_ms = meta.created().map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64).unwrap_or(0);
        let modified_at_ms = meta.modified().map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64).unwrap_or(0);
        Ok(FileMetadata { is_directory, is_file, created_at_ms, modified_at_ms })
    }

    async fn read_directory(&self, path: &AbsolutePathBuf) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let is_directory = meta.is_dir();
            let is_file = meta.is_file();
            let file_name = entry.file_name().to_string_lossy().to_string();
            entries.push(ReadDirectoryEntry { file_name, is_directory, is_file });
        }
        Ok(entries)
    }

    async fn remove(&self, path: &AbsolutePathBuf, options: RemoveOptions) -> FileSystemResult<()> {
        let meta = std::fs::metadata(path)?;
        if meta.is_dir() {
            if options.recursive {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_dir(path)
            }
        } else {
            std::fs::remove_file(path)
        }
    }

    async fn copy(&self, source: &AbsolutePathBuf, dest: &AbsolutePathBuf, _options: CopyOptions) -> FileSystemResult<()> {
        std::fs::copy(source, dest)?;
        Ok(())
    }
}

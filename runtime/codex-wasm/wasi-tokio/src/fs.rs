//! Filesystem operations matching tokio::fs.
//!
//! In wasip2, these route through wasi:filesystem which is shimmed
//! to OPFS in the browser or native FS on iOS.

use std::io;
use std::path::Path;

pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    std::fs::read_to_string(path)
}

pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_file(path)
}

pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

pub async fn metadata(path: impl AsRef<Path>) -> io::Result<std::fs::Metadata> {
    std::fs::metadata(path)
}

pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    std::fs::rename(from, to)
}

pub async fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    std::fs::copy(from, to)
}

pub async fn canonicalize(path: impl AsRef<Path>) -> io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path)
}

pub async fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
    let inner = std::fs::read_dir(path)?;
    Ok(ReadDir { inner })
}

/// Async ReadDir matching tokio::fs::ReadDir.
pub struct ReadDir {
    inner: std::fs::ReadDir,
}

impl ReadDir {
    pub async fn next_entry(&mut self) -> io::Result<Option<DirEntry>> {
        match self.inner.next() {
            Some(Ok(entry)) => Ok(Some(DirEntry { inner: entry })),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }
}

/// Async DirEntry matching tokio::fs::DirEntry.
pub struct DirEntry {
    inner: std::fs::DirEntry,
}

impl DirEntry {
    pub fn path(&self) -> std::path::PathBuf {
        self.inner.path()
    }

    pub fn file_name(&self) -> std::ffi::OsString {
        self.inner.file_name()
    }

    pub async fn metadata(&self) -> io::Result<std::fs::Metadata> {
        self.inner.metadata()
    }

    pub async fn file_type(&self) -> io::Result<std::fs::FileType> {
        self.inner.file_type()
    }
}

pub async fn try_exists(path: impl AsRef<Path>) -> io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub async fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

pub async fn read_link(path: impl AsRef<Path>) -> io::Result<std::path::PathBuf> {
    std::fs::read_link(path)
}

pub async fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// File type matching tokio::fs::File.
pub struct File {
    inner: std::fs::File,
}

impl File {
    pub async fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            inner: std::fs::File::open(path)?,
        })
    }

    pub async fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            inner: std::fs::File::create(path)?,
        })
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        use std::io::Write;
        self.inner.flush()
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.inner.write_all(buf)
    }

    pub fn into_std(self) -> std::fs::File {
        self.inner
    }

    pub fn from_std(std: std::fs::File) -> Self {
        Self { inner: std }
    }

    pub async fn metadata(&self) -> io::Result<std::fs::Metadata> {
        self.inner.metadata()
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use std::io::Read;
        self.inner.read(buf)
    }

    pub async fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        use std::io::Read;
        self.inner.read_to_end(buf)
    }

    pub async fn set_permissions(&self, perm: std::fs::Permissions) -> io::Result<()> {
        self.inner.set_permissions(perm)
    }
}

impl std::io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl crate::io::AsyncRead for File {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut crate::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::io::Read;
        let unfilled = buf.unfilled_mut();
        match self.inner.read(unfilled) {
            Ok(n) => {
                buf.advance(n);
                std::task::Poll::Ready(Ok(()))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }
}

impl crate::io::AsyncWrite for File {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::io::Write;
        match self.inner.write(buf) {
            Ok(n) => std::task::Poll::Ready(Ok(n)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::task::Poll::Pending,
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::io::Write;
        std::task::Poll::Ready(self.inner.flush())
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// OpenOptions matching tokio::fs::OpenOptions.
pub struct OpenOptions {
    inner: std::fs::OpenOptions,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self {
            inner: std::fs::OpenOptions::new(),
        }
    }

    pub fn read(&mut self, read: bool) -> &mut Self {
        self.inner.read(read);
        self
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        self.inner.write(write);
        self
    }

    pub fn create(&mut self, create: bool) -> &mut Self {
        self.inner.create(create);
        self
    }

    pub fn append(&mut self, append: bool) -> &mut Self {
        self.inner.append(append);
        self
    }

    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.inner.truncate(truncate);
        self
    }

    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.inner.create_new(create_new);
        self
    }

    pub async fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        Ok(File {
            inner: self.inner.open(path)?,
        })
    }
}

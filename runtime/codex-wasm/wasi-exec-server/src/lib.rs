//! Exec-server for wasip2 — backend injection model.
//!
//! The component entry point (codex-wasm-tui) registers a concrete ExecBackend
//! via `set_exec_backend()` at startup. This backend routes through the WIT
//! shell-pty interface to the browser host for persistent PTY sessions.
#![allow(dead_code, unused_variables, unused_imports)]
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use serde::{Deserialize, Serialize};

pub mod wasi_fs {
    use std::io;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use crate::{CopyOptions, CreateDirectoryOptions, ExecutorFileSystem, FileMetadata, FileSystemResult, ReadDirectoryEntry, RemoveOptions};

    pub struct WasiFs;

    #[async_trait::async_trait]
    impl ExecutorFileSystem for WasiFs {
        async fn read_file(&self, path: &AbsolutePathBuf) -> FileSystemResult<Vec<u8>> {
            std::fs::read(path).map_err(Into::into)
        }

        async fn write_file(&self, path: &AbsolutePathBuf, contents: Vec<u8>) -> FileSystemResult<()> {
            std::fs::write(path, contents).map_err(Into::into)
        }

        async fn create_directory(&self, path: &AbsolutePathBuf, options: CreateDirectoryOptions) -> FileSystemResult<()> {
            if options.recursive {
                std::fs::create_dir_all(path).map_err(Into::into)
            } else {
                std::fs::create_dir(path).map_err(Into::into)
            }
        }

        async fn get_metadata(&self, path: &AbsolutePathBuf) -> FileSystemResult<FileMetadata> {
            let meta = std::fs::metadata(path).map_err(Into::<io::Error>::into)?;
            let is_directory = meta.is_dir();
            let is_file = meta.is_file();
            let created_at_ms = meta.created().map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64).unwrap_or(0);
            let modified_at_ms = meta.modified().map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64).unwrap_or(0);
            Ok(FileMetadata { is_directory, is_file, created_at_ms, modified_at_ms })
        }

        async fn read_directory(&self, path: &AbsolutePathBuf) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(path).map_err(Into::<io::Error>::into)? {
                let entry = entry.map_err(Into::<io::Error>::into)?;
                let meta = entry.metadata().map_err(Into::<io::Error>::into)?;
                let is_directory = meta.is_dir();
                let is_file = meta.is_file();
                let file_name = entry.file_name().to_string_lossy().to_string();
                entries.push(ReadDirectoryEntry { file_name, is_directory, is_file });
            }
            Ok(entries)
        }

        async fn remove(&self, path: &AbsolutePathBuf, options: RemoveOptions) -> FileSystemResult<()> {
            let meta = std::fs::metadata(path).map_err(Into::<io::Error>::into)?;
            if meta.is_dir() {
                if options.recursive {
                    std::fs::remove_dir_all(path).map_err(Into::into)
                } else {
                    std::fs::remove_dir(path).map_err(Into::into)
                }
            } else {
                std::fs::remove_file(path).map_err(Into::into)
            }
        }

        async fn copy(&self, source: &AbsolutePathBuf, dest: &AbsolutePathBuf, _options: CopyOptions) -> FileSystemResult<()> {
            std::fs::copy(source, dest).map_err(Into::<io::Error>::into)?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcessId(String);
impl ProcessId {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
    pub fn into_inner(self) -> String { self.0 }
}
impl std::ops::Deref for ProcessId { type Target = str; fn deref(&self) -> &str { self.as_str() } }
impl std::borrow::Borrow<str> for ProcessId { fn borrow(&self) -> &str { self.as_str() } }
impl AsRef<str> for ProcessId { fn as_ref(&self) -> &str { self.as_str() } }
impl std::fmt::Display for ProcessId { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) } }
impl From<String> for ProcessId { fn from(value: String) -> Self { Self(value) } }
impl From<&str> for ProcessId { fn from(value: &str) -> Self { Self(value.to_string()) } }
impl From<&String> for ProcessId { fn from(value: &String) -> Self { Self(value.clone()) } }
impl From<ProcessId> for String { fn from(value: ProcessId) -> Self { value.0 } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteChunk(pub Vec<u8>);
impl ByteChunk { pub fn into_inner(self) -> Vec<u8> { self.0 } }
impl From<Vec<u8>> for ByteChunk { fn from(v: Vec<u8>) -> Self { Self(v) } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams { pub client_name: String }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResponse {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecParams { pub process_id: ProcessId, pub argv: Vec<String>, pub cwd: PathBuf, pub env: HashMap<String, String>, pub tty: bool, pub arg0: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResponse { pub process_id: ProcessId }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadParams { pub process_id: String, pub after_seq: Option<u64>, pub max_bytes: Option<usize>, pub wait_ms: Option<u64> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutputChunk { pub seq: u64, pub stream: ExecOutputStream, pub chunk: ByteChunk }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResponse { pub chunks: Vec<ProcessOutputChunk>, pub next_seq: u64, pub exited: bool, pub exit_code: Option<i32>, pub closed: bool, pub failure: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteParams { pub process_id: String, pub chunk: ByteChunk }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteStatus { Accepted, UnknownProcess, StdinClosed, Starting }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteResponse { pub status: WriteStatus }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminateParams { pub process_id: String }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminateResponse { pub running: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecOutputStream { Stdout, Stderr, Pty }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecOutputDeltaNotification { pub process_id: String, pub stream: ExecOutputStream, pub chunk: ByteChunk }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecExitedNotification { pub process_id: String, pub exit_code: i32 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecServerEvent { OutputDelta(ExecOutputDeltaNotification), Exited(ExecExitedNotification) }

#[derive(Debug)]
pub enum ExecServerError {
    Spawn(io::Error),
    Closed,
    Json(serde_json::Error),
    Protocol(String),
    Server { code: i64, message: String },
}
impl std::fmt::Display for ExecServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "ExecServerError") }
}
impl std::error::Error for ExecServerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecServerClientConnectOptions { pub client_name: String, pub initialize_timeout: Duration }
impl Default for ExecServerClientConnectOptions {
    fn default() -> Self { Self { client_name: "codex".into(), initialize_timeout: Duration::from_secs(10) } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecServerConnectArgs { pub websocket_url: String, pub client_name: String, pub connect_timeout: Duration, pub initialize_timeout: Duration }
impl RemoteExecServerConnectArgs {
    pub fn new(websocket_url: String, client_name: String) -> Self { Self { websocket_url, client_name, connect_timeout: Duration::from_secs(10), initialize_timeout: Duration::from_secs(10) } }
}
impl From<RemoteExecServerConnectArgs> for ExecServerClientConnectOptions {
    fn from(a: RemoteExecServerConnectArgs) -> Self { Self { client_name: a.client_name, initialize_timeout: a.initialize_timeout } }
}

pub struct ExecServerClient;
impl ExecServerClient {
    pub async fn connect_websocket(_args: RemoteExecServerConnectArgs) -> Result<Self, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub fn event_receiver(&self) -> tokio::sync::broadcast::Receiver<ExecServerEvent> { let (_tx, rx) = tokio::sync::broadcast::channel(1); rx }
    pub async fn initialize(&self, _options: ExecServerClientConnectOptions) -> Result<InitializeResponse, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub async fn exec(&self, _params: ExecParams) -> Result<ExecResponse, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub async fn read(&self, _params: ReadParams) -> Result<ReadResponse, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub async fn write(&self, _process_id: &str, _chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub async fn terminate(&self, _process_id: &str) -> Result<TerminateResponse, ExecServerError> { Err(ExecServerError::Protocol("not available in WASM".into())) }
    pub async fn notify_initialized(&self) -> Result<(), ExecServerError> { Ok(()) }
}

pub struct StartedExecProcess {
    pub process: Arc<dyn ExecProcess>,
}

#[async_trait::async_trait]
pub trait ExecProcess: Send + Sync {
    fn process_id(&self) -> &ProcessId;
    fn subscribe_wake(&self) -> tokio::sync::watch::Receiver<u64>;
    async fn read(&self, after_seq: Option<u64>, max_bytes: Option<usize>, wait_ms: Option<u64>) -> Result<ReadResponse, ExecServerError>;
    async fn write(&self, chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError>;
    async fn terminate(&self) -> Result<(), ExecServerError>;
}

#[async_trait::async_trait]
pub trait ExecBackend: Send + Sync {
    async fn start(&self, params: ExecParams) -> Result<StartedExecProcess, ExecServerError>;
}

pub trait ExecutorEnvironment: Send + Sync {
    fn get_exec_backend(&self) -> Arc<dyn ExecBackend>;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CreateDirectoryOptions { pub recursive: bool }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RemoveOptions { pub recursive: bool, pub force: bool }
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CopyOptions { pub recursive: bool }
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FileMetadata { pub is_directory: bool, pub is_file: bool, pub created_at_ms: i64, pub modified_at_ms: i64 }
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadDirectoryEntry { pub file_name: String, pub is_directory: bool, pub is_file: bool }
pub type FileSystemResult<T> = io::Result<T>;

/// Stub SandboxPolicy — WASM has no sandbox policy enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy;

#[async_trait::async_trait]
pub trait ExecutorFileSystem: Send + Sync {
    async fn read_file(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<u8>>;

    /// Reads a file and decodes it as UTF-8 text.
    async fn read_file_text(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<String> {
        let bytes = self.read_file(path).await?;
        String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn read_file_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<Vec<u8>> {
        self.read_file(path).await
    }

    async fn write_file(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, contents: Vec<u8>) -> FileSystemResult<()>;

    async fn write_file_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        contents: Vec<u8>,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<()> {
        self.write_file(path, contents).await
    }

    async fn create_directory(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, options: CreateDirectoryOptions) -> FileSystemResult<()>;

    async fn create_directory_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        create_directory_options: CreateDirectoryOptions,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<()> {
        self.create_directory(path, create_directory_options).await
    }

    async fn get_metadata(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<FileMetadata>;

    async fn get_metadata_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<FileMetadata> {
        self.get_metadata(path).await
    }

    async fn read_directory(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<ReadDirectoryEntry>>;

    async fn read_directory_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<Vec<ReadDirectoryEntry>> {
        self.read_directory(path).await
    }

    async fn remove(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, options: RemoveOptions) -> FileSystemResult<()>;

    async fn remove_with_sandbox_policy(
        &self,
        path: &codex_utils_absolute_path::AbsolutePathBuf,
        remove_options: RemoveOptions,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<()> {
        self.remove(path, remove_options).await
    }

    async fn copy(&self, source: &codex_utils_absolute_path::AbsolutePathBuf, dest: &codex_utils_absolute_path::AbsolutePathBuf, options: CopyOptions) -> FileSystemResult<()>;

    async fn copy_with_sandbox_policy(
        &self,
        source_path: &codex_utils_absolute_path::AbsolutePathBuf,
        destination_path: &codex_utils_absolute_path::AbsolutePathBuf,
        copy_options: CopyOptions,
        _sandbox_policy: Option<&SandboxPolicy>,
    ) -> FileSystemResult<()> {
        self.copy(source_path, destination_path, copy_options).await
    }
}

/// Global exec backend, set once at component startup via `set_exec_backend()`.
static EXEC_BACKEND: OnceLock<Arc<dyn ExecBackend>> = OnceLock::new();

/// Register the exec backend. Called once by the component entry point.
/// Must be called before any `Environment` is created.
pub fn set_exec_backend(backend: Arc<dyn ExecBackend>) {
    let _ = EXEC_BACKEND.set(backend);
}

/// Get the registered exec backend, or a stub if none registered.
fn get_registered_backend() -> Arc<dyn ExecBackend> {
    EXEC_BACKEND
        .get()
        .cloned()
        .unwrap_or_else(|| Arc::new(StubBackend))
}

pub struct EnvironmentManager {
    exec_server_url: Option<String>,
}
impl EnvironmentManager {
    pub fn new(exec_server_url: Option<String>) -> Self { Self { exec_server_url } }
    pub fn from_env() -> Self { Self::new(std::env::var("CODEX_EXEC_SERVER_URL").ok()) }
    pub fn from_environment(environment: Option<&Environment>) -> Self {
        match environment {
            Some(env) => Self { exec_server_url: env.exec_server_url().map(str::to_owned) },
            None => Self { exec_server_url: None },
        }
    }
    pub fn exec_server_url(&self) -> Option<&str> { self.exec_server_url.as_deref() }
    pub fn is_remote(&self) -> bool { self.exec_server_url.is_some() }
    pub async fn current(&self) -> Result<Option<Arc<Environment>>, ExecServerError> { Ok(Some(Arc::new(Environment::create(self.exec_server_url.clone()).await?))) }
}

pub struct Environment {
    exec_server_url: Option<String>,
}
impl Default for Environment { fn default() -> Self { Self { exec_server_url: Some("wasm-host".to_string()) } } }
impl std::fmt::Debug for Environment { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("Environment").finish() } }
impl Environment {
    pub async fn create(_url: Option<String>) -> Result<Self, ExecServerError> {
        // Always use "wasm-host" sentinel so upstream takes the remote exec path
        Ok(Self { exec_server_url: Some("wasm-host".to_string()) })
    }
    pub fn is_remote(&self) -> bool { self.exec_server_url.is_some() }
    pub fn exec_server_url(&self) -> Option<&str> { self.exec_server_url.as_deref() }
    pub fn get_exec_backend(&self) -> Arc<dyn ExecBackend> { get_registered_backend() }
    pub fn get_filesystem(&self) -> Arc<dyn ExecutorFileSystem> { Arc::new(wasi_fs::WasiFs) }
}
impl ExecutorEnvironment for Environment { fn get_exec_backend(&self) -> Arc<dyn ExecBackend> { get_registered_backend() } }

struct StubBackend;
#[async_trait::async_trait]
impl ExecBackend for StubBackend {
    async fn start(&self, _: ExecParams) -> Result<StartedExecProcess, ExecServerError> {
        Err(ExecServerError::Protocol("No exec backend registered — call codex_exec_server::set_exec_backend() first".into()))
    }
}

struct StubExec;
static STUB_PROCESS_ID: std::sync::LazyLock<ProcessId> = std::sync::LazyLock::new(|| ProcessId::new("stub"));
#[async_trait::async_trait]
impl ExecProcess for StubExec {
    fn process_id(&self) -> &ProcessId { &STUB_PROCESS_ID }
    fn subscribe_wake(&self) -> tokio::sync::watch::Receiver<u64> { let (_tx, rx) = tokio::sync::watch::channel(0); rx }
    async fn read(&self, _: Option<u64>, _: Option<usize>, _: Option<u64>) -> Result<ReadResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    async fn write(&self, _: Vec<u8>) -> Result<WriteResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    async fn terminate(&self) -> Result<(), ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
}

struct StubFs;
#[async_trait::async_trait]
impl ExecutorFileSystem for StubFs {
    async fn read_file(&self, _: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<u8>> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn write_file(&self, _: &codex_utils_absolute_path::AbsolutePathBuf, _: Vec<u8>) -> FileSystemResult<()> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn create_directory(&self, _: &codex_utils_absolute_path::AbsolutePathBuf, _: CreateDirectoryOptions) -> FileSystemResult<()> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn get_metadata(&self, _: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<FileMetadata> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn read_directory(&self, _: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<ReadDirectoryEntry>> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn remove(&self, _: &codex_utils_absolute_path::AbsolutePathBuf, _: RemoveOptions) -> FileSystemResult<()> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
    async fn copy(&self, _: &codex_utils_absolute_path::AbsolutePathBuf, _: &codex_utils_absolute_path::AbsolutePathBuf, _: CopyOptions) -> FileSystemResult<()> { Err(io::Error::new(io::ErrorKind::Unsupported, "WASM")) }
}

// Protocol method constants
pub const DEFAULT_LISTEN_URL: &str = "ws://127.0.0.1:0";
pub const INITIALIZE_METHOD: &str = "initialize";
pub const INITIALIZED_METHOD: &str = "initialized";
pub const EXEC_METHOD: &str = "process/start";
pub const EXEC_READ_METHOD: &str = "process/read";
pub const EXEC_WRITE_METHOD: &str = "process/write";
pub const EXEC_TERMINATE_METHOD: &str = "process/terminate";
pub const EXEC_OUTPUT_DELTA_METHOD: &str = "process/output";
pub const EXEC_EXITED_METHOD: &str = "process/exited";

/// Global LOCAL_FS instance used by apply-patch tests and standalone_executable.
pub static LOCAL_FS: std::sync::LazyLock<Arc<dyn ExecutorFileSystem>> =
    std::sync::LazyLock::new(|| Arc::new(wasi_fs::WasiFs));

// Re-export FS types that codex-core may need (originally from codex-app-server-protocol)
// These are defined inline since we stub the re-exports

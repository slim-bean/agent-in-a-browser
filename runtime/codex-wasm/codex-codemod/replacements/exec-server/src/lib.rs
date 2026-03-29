//! Stub — exec-server for wasip2.
#![allow(dead_code, unused_variables, unused_imports)]
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteChunk(pub Vec<u8>);
impl ByteChunk { pub fn into_inner(self) -> Vec<u8> { self.0 } }
impl From<Vec<u8>> for ByteChunk { fn from(v: Vec<u8>) -> Self { Self(v) } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams { pub client_name: String }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResponse {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecParams { pub process_id: String, pub argv: Vec<String>, pub cwd: PathBuf, pub env: HashMap<String, String>, pub tty: bool, pub arg0: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResponse { pub process_id: String }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadParams { pub process_id: String, pub after_seq: Option<u64>, pub max_bytes: Option<usize>, pub wait_ms: Option<u64> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutputChunk { pub seq: u64, pub stream: ExecOutputStream, pub chunk: ByteChunk }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResponse { pub chunks: Vec<ProcessOutputChunk>, pub next_seq: u64, pub exited: bool, pub exit_code: Option<i32> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteParams { pub process_id: String, pub chunk: ByteChunk }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteResponse { pub accepted: bool }
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

#[async_trait::async_trait]
pub trait ExecProcess: Send + Sync {
    async fn start(&self, params: ExecParams) -> Result<ExecResponse, ExecServerError>;
    async fn read(&self, params: ReadParams) -> Result<ReadResponse, ExecServerError>;
    async fn write(&self, process_id: &str, chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError>;
    async fn terminate(&self, process_id: &str) -> Result<TerminateResponse, ExecServerError>;
    fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ExecServerEvent>;
}

pub trait ExecutorEnvironment: Send + Sync {
    fn get_executor(&self) -> Arc<dyn ExecProcess>;
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

#[async_trait::async_trait]
pub trait ExecutorFileSystem: Send + Sync {
    async fn read_file(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<u8>>;
    async fn write_file(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, contents: Vec<u8>) -> FileSystemResult<()>;
    async fn create_directory(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, options: CreateDirectoryOptions) -> FileSystemResult<()>;
    async fn get_metadata(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<FileMetadata>;
    async fn read_directory(&self, path: &codex_utils_absolute_path::AbsolutePathBuf) -> FileSystemResult<Vec<ReadDirectoryEntry>>;
    async fn remove(&self, path: &codex_utils_absolute_path::AbsolutePathBuf, options: RemoveOptions) -> FileSystemResult<()>;
    async fn copy(&self, source: &codex_utils_absolute_path::AbsolutePathBuf, dest: &codex_utils_absolute_path::AbsolutePathBuf, options: CopyOptions) -> FileSystemResult<()>;
}

pub struct Environment;
impl Default for Environment { fn default() -> Self { Self } }
impl std::fmt::Debug for Environment { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("Environment").finish() } }
impl Environment {
    pub async fn create(_url: Option<String>) -> Result<Self, ExecServerError> { Ok(Self) }
    pub fn experimental_exec_server_url(&self) -> Option<&str> { None }
    pub fn get_executor(&self) -> Arc<dyn ExecProcess> { Arc::new(StubExec) }
    pub fn get_filesystem(&self) -> Arc<dyn ExecutorFileSystem> { Arc::new(StubFs) }
}
impl ExecutorEnvironment for Environment { fn get_executor(&self) -> Arc<dyn ExecProcess> { Arc::new(StubExec) } }

struct StubExec;
#[async_trait::async_trait]
impl ExecProcess for StubExec {
    async fn start(&self, _: ExecParams) -> Result<ExecResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    async fn read(&self, _: ReadParams) -> Result<ReadResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    async fn write(&self, _: &str, _: Vec<u8>) -> Result<WriteResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    async fn terminate(&self, _: &str) -> Result<TerminateResponse, ExecServerError> { Err(ExecServerError::Protocol("WASM".into())) }
    fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ExecServerEvent> { let (_tx, rx) = tokio::sync::broadcast::channel(1); rx }
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

// Re-export FS types that codex-core may need (originally from codex-app-server-protocol)
// These are defined inline since we stub the re-exports

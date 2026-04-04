//! PTY backend that routes through the WIT shell-pty interface.
//!
//! Implements `ExecBackend` and `ExecProcess` from codex-exec-server by
//! delegating to the host (browser SharedWorker) via WIT imports. The host
//! maintains persistent shell sessions with stateful `ShellEnv` instances.

use std::sync::Arc;

use crate::bindings::codex::tui::shell_pty;
use codex_exec_server::*;

/// ExecBackend that starts processes via the WIT shell-pty interface.
pub struct WitPtyBackend;

#[async_trait::async_trait]
impl ExecBackend for WitPtyBackend {
    async fn start(&self, params: ExecParams) -> Result<StartedExecProcess, ExecServerError> {
        let wit_params = shell_pty::PtyStartParams {
            process_id: params.process_id.as_str().to_string(),
            argv: params.argv,
            cwd: params.cwd.to_string_lossy().to_string(),
            env: params.env.into_iter().collect(),
            tty: params.tty,
        };

        let result = shell_pty::start(&wit_params).map_err(|e| ExecServerError::Protocol(e))?;

        let process = Arc::new(WitPtyProcess::new(result.process_id));
        Ok(StartedExecProcess { process })
    }
}

/// ExecProcess that communicates with a persistent host-side shell session.
struct WitPtyProcess {
    process_id: ProcessId,
    wake_tx: tokio::sync::watch::Sender<u64>,
    wake_rx: tokio::sync::watch::Receiver<u64>,
}

impl WitPtyProcess {
    fn new(process_id: String) -> Self {
        let (wake_tx, wake_rx) = tokio::sync::watch::channel(0u64);
        let pid = ProcessId::new(process_id.clone());

        // Spawn a background task that polls the host for wake notifications.
        // When the host has new output, the wake sequence increments and the
        // upstream output-reading task (spawned by from_remote_started) will
        // call read() to collect the output.
        let poll_pid = process_id;
        let poll_tx = wake_tx.clone();
        tokio::spawn(async move {
            loop {
                // poll-wake is a JSPI-suspendable import — it yields to the
                // JS event loop and returns when the host has an update.
                match shell_pty::poll_wake(&poll_pid) {
                    Ok(seq) => {
                        let _ = poll_tx.send(seq);
                        if seq == u64::MAX {
                            // Sentinel: process has exited, stop polling
                            break;
                        }
                    }
                    Err(_) => {
                        // Process gone or error — stop polling
                        break;
                    }
                }
                // Small yield to avoid tight-looping
                tokio::task::yield_now().await;
            }
        });

        Self {
            process_id: pid,
            wake_tx,
            wake_rx,
        }
    }
}

#[async_trait::async_trait]
impl ExecProcess for WitPtyProcess {
    fn process_id(&self) -> &ProcessId {
        &self.process_id
    }

    fn subscribe_wake(&self) -> tokio::sync::watch::Receiver<u64> {
        self.wake_rx.clone()
    }

    async fn read(
        &self,
        after_seq: Option<u64>,
        max_bytes: Option<usize>,
        wait_ms: Option<u64>,
    ) -> Result<ReadResponse, ExecServerError> {
        let result = shell_pty::read(
            self.process_id.as_str(),
            after_seq,
            max_bytes.map(|b| b as u32),
            wait_ms,
        )
        .map_err(|e| ExecServerError::Protocol(e))?;

        let chunks = result
            .chunks
            .into_iter()
            .map(|chunk| ProcessOutputChunk {
                seq: chunk.seq,
                stream: ExecOutputStream::Pty,
                chunk: ByteChunk(chunk.data),
            })
            .collect();

        Ok(ReadResponse {
            chunks,
            next_seq: result.next_seq,
            exited: result.exited,
            exit_code: result.exit_code,
            closed: result.closed,
            failure: result.failure,
        })
    }

    async fn write(&self, chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError> {
        let result = shell_pty::write(self.process_id.as_str(), &chunk)
            .map_err(|e| ExecServerError::Protocol(e))?;

        let status = match result.status {
            shell_pty::WriteStatus::Accepted => WriteStatus::Accepted,
            shell_pty::WriteStatus::UnknownProcess => WriteStatus::UnknownProcess,
            shell_pty::WriteStatus::StdinClosed => WriteStatus::StdinClosed,
            shell_pty::WriteStatus::Starting => WriteStatus::Starting,
        };

        Ok(WriteResponse { status })
    }

    async fn terminate(&self) -> Result<(), ExecServerError> {
        shell_pty::terminate(self.process_id.as_str()).map_err(|e| ExecServerError::Protocol(e))?;
        // Signal wake polling to stop
        let _ = self.wake_tx.send(u64::MAX);
        Ok(())
    }
}

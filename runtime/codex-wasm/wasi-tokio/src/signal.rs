//! Signal handling matching tokio::signal.
//!
//! No signals in WASM — these are all no-ops.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub mod unix {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub enum SignalKind {
        Interrupt,
        Terminate,
        HangUp,
    }

    impl SignalKind {
        pub fn interrupt() -> Self {
            Self::Interrupt
        }

        pub fn terminate() -> Self {
            Self::Terminate
        }

        pub fn hangup() -> Self {
            Self::HangUp
        }

        pub fn from_raw(_signum: i32) -> Self {
            Self::Interrupt
        }
    }

    pub struct Signal {
        _kind: SignalKind,
    }

    impl Signal {
        pub async fn recv(&mut self) -> Option<()> {
            // Never fires in WASM — pend forever
            std::future::pending().await
        }
    }

    pub fn signal(kind: SignalKind) -> std::io::Result<Signal> {
        Ok(Signal { _kind: kind })
    }
}

/// ctrl_c() matching tokio::signal::ctrl_c().
pub async fn ctrl_c() -> std::io::Result<()> {
    // Never fires in WASM
    std::future::pending().await
}

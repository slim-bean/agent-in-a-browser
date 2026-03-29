//! Execute macro matching crossterm::execute! and crossterm::queue!

/// Execute commands immediately (flush after each).
#[macro_export]
macro_rules! execute {
    ($writer:expr $(, $command:expr)* $(,)?) => {{
        use $crate::ExecutableCommand as _;
        $(
            ($writer).execute($command)?;
        )*
        Ok::<(), std::io::Error>(())
    }};
}

/// Queue commands (write without flush).
#[macro_export]
macro_rules! queue {
    ($writer:expr $(, $command:expr)* $(,)?) => {{
        use $crate::QueueableCommand as _;
        $(
            ($writer).queue($command)?;
        )*
        Ok::<(), std::io::Error>(())
    }};
}

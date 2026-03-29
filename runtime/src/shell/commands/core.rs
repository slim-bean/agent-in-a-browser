//! Core shell commands: echo, pwd, true, false, yes, help

use futures_lite::io::AsyncWriteExt;
use runtime_macros::shell_commands;
use std::io;

use super::super::ShellEnv;
use super::{parse_common, ShellCommands};

/// Core commands - basic shell utilities.
pub struct CoreCommands;

#[shell_commands]
impl CoreCommands {
    /// echo - output arguments
    #[shell_command(
        name = "echo",
        usage = "echo [-e] [-n] [STRING]...",
        description = "Display line of text"
    )]
    fn cmd_echo(
        args: Vec<String>,
        _env: &ShellEnv,
        _stdin: piper::Reader,
        mut stdout: piper::Writer,
        _stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Box::pin(async move {
            let (_, remaining) = parse_common(&args);
            // Parse echo-specific flags
            let mut interpret_escapes = false;
            let mut trailing_newline = true;
            let mut args_to_print: Vec<&str> = Vec::new();

            for arg in &remaining {
                if arg == "-e" {
                    interpret_escapes = true;
                } else if arg == "-n" {
                    trailing_newline = false;
                } else if arg == "-E" {
                    interpret_escapes = false;
                } else if arg.starts_with('-')
                    && arg
                        .chars()
                        .skip(1)
                        .all(|c| c == 'e' || c == 'n' || c == 'E')
                {
                    // Combined flags like -en
                    for c in arg.chars().skip(1) {
                        match c {
                            'e' => interpret_escapes = true,
                            'n' => trailing_newline = false,
                            'E' => interpret_escapes = false,
                            _ => {}
                        }
                    }
                } else {
                    args_to_print.push(arg);
                }
            }

            let mut output = args_to_print.join(" ");

            // Handle escape sequences if -e is specified
            if interpret_escapes {
                output = output
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\r", "\r")
                    .replace("\\\\", "\\")
                    .replace("\\0", "\0");
            }

            if stdout.write_all(output.as_bytes()).await.is_err() {
                return 1;
            }
            if trailing_newline {
                if stdout.write_all(b"\n").await.is_err() {
                    return 1;
                }
            }
            0
        })
    }

    /// pwd - print working directory
    #[shell_command(
        name = "pwd",
        usage = "pwd",
        description = "Print current working directory"
    )]
    fn cmd_pwd(
        args: Vec<String>,
        env: &ShellEnv,
        _stdin: piper::Reader,
        mut stdout: piper::Writer,
        _stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        let cwd = env.cwd.to_string_lossy().to_string();
        Box::pin(async move {
            let (_, _) = parse_common(&args);
            if stdout.write_all(cwd.as_bytes()).await.is_err() {
                return 1;
            }
            if stdout.write_all(b"\n").await.is_err() {
                return 1;
            }
            0
        })
    }

    /// yes - output "y" repeatedly (handles BrokenPipe gracefully)
    #[shell_command(
        name = "yes",
        usage = "yes [STRING]",
        description = "Output a string repeatedly until killed"
    )]
    fn cmd_yes(
        args: Vec<String>,
        _env: &ShellEnv,
        _stdin: piper::Reader,
        mut stdout: piper::Writer,
        _stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        let (_, remaining) = parse_common(&args);
        let output = if remaining.is_empty() {
            "y".to_string()
        } else {
            remaining.join(" ")
        };

        Box::pin(async move {
            let line = format!("{}\n", output);
            loop {
                match stdout.write_all(line.as_bytes()).await {
                    Ok(_) => continue,
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return 0,
                    Err(_) => return 1,
                }
            }
        })
    }

    /// true - exit with 0
    /// sh / bash / /bin/sh / /bin/bash - re-entrant shell execution
    ///
    /// When the model or upstream code runs `/bin/sh -c "command"` or
    /// `/bin/sh -lc "command"`, we extract the command and execute it
    /// through our own shell pipeline. This is critical for the codex
    /// TUI which wraps all tool calls in `/bin/sh -lc <command>`.
    #[shell_command(
        name = "sh",
        usage = "sh [-c|-lc] COMMAND",
        description = "Execute a command through the shell"
    )]
    fn cmd_sh(
        args: Vec<String>,
        env: &ShellEnv,
        _stdin: piper::Reader,
        mut stdout: piper::Writer,
        mut stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        let env_clone = env.clone();
        Box::pin(async move {
            // Parse args: sh [-c|-lc] command_string
            let mut i = 0;
            let mut login = false;
            let mut command_str: Option<String> = None;

            while i < args.len() {
                match args[i].as_str() {
                    "-c" => {
                        // Everything after -c is the command
                        if i + 1 < args.len() {
                            command_str = Some(args[i + 1..].join(" "));
                        }
                        break;
                    }
                    "-lc" => {
                        login = true;
                        if i + 1 < args.len() {
                            command_str = Some(args[i + 1..].join(" "));
                        }
                        break;
                    }
                    "-l" => {
                        login = true;
                        i += 1;
                    }
                    _ => {
                        // Treat as a script file path (not supported, just run as command)
                        command_str = Some(args[i..].join(" "));
                        break;
                    }
                }
            }

            let _ = login; // login flag acknowledged but no special handling needed

            if let Some(cmd) = command_str {
                if cmd.is_empty() {
                    return 0;
                }
                let mut env_mut = env_clone;
                let result = super::super::run_pipeline(&cmd, &mut env_mut).await;
                if !result.stdout.is_empty() {
                    let _ = stdout.write_all(result.stdout.as_bytes()).await;
                }
                if !result.stderr.is_empty() {
                    let _ = stderr.write_all(result.stderr.as_bytes()).await;
                }
                result.code
            } else {
                // No command provided — in interactive mode we'd start a REPL,
                // but for tool execution just return success
                0
            }
        })
    }

    #[shell_command(
        name = "bash",
        usage = "bash [-c|-lc] COMMAND",
        description = "Execute a command through the shell (bash alias)"
    )]
    fn cmd_bash(
        args: Vec<String>,
        env: &ShellEnv,
        stdin: piper::Reader,
        stdout: piper::Writer,
        stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Self::cmd_sh(args, env, stdin, stdout, stderr)
    }

    #[shell_command(
        name = "/bin/sh",
        usage = "/bin/sh [-c|-lc] COMMAND",
        description = "Execute a command through the shell (absolute path)"
    )]
    fn cmd_bin_sh(
        args: Vec<String>,
        env: &ShellEnv,
        stdin: piper::Reader,
        stdout: piper::Writer,
        stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Self::cmd_sh(args, env, stdin, stdout, stderr)
    }

    #[shell_command(
        name = "/bin/bash",
        usage = "/bin/bash [-c|-lc] COMMAND",
        description = "Execute a command through the shell (absolute path)"
    )]
    fn cmd_bin_bash(
        args: Vec<String>,
        env: &ShellEnv,
        stdin: piper::Reader,
        stdout: piper::Writer,
        stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Self::cmd_sh(args, env, stdin, stdout, stderr)
    }

    /// true - exit with 0
    #[shell_command(
        name = "true",
        usage = "true",
        description = "Do nothing, successfully"
    )]
    fn cmd_true(
        args: Vec<String>,
        _env: &ShellEnv,
        _stdin: piper::Reader,
        _stdout: piper::Writer,
        _stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Box::pin(async move {
            let (_, _) = parse_common(&args);
            0
        })
    }

    /// false - exit with 1
    #[shell_command(
        name = "false",
        usage = "false",
        description = "Do nothing, unsuccessfully"
    )]
    fn cmd_false(
        args: Vec<String>,
        _env: &ShellEnv,
        _stdin: piper::Reader,
        _stdout: piper::Writer,
        _stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Box::pin(async move {
            let (_, _) = parse_common(&args);
            1
        })
    }

    /// help - display available commands or help for a specific command
    #[shell_command(
        name = "help",
        usage = "help [COMMAND]",
        description = "Display available commands or help for a specific command"
    )]
    fn cmd_help(
        args: Vec<String>,
        _env: &ShellEnv,
        _stdin: piper::Reader,
        mut stdout: piper::Writer,
        mut stderr: piper::Writer,
    ) -> futures_lite::future::Boxed<i32> {
        Box::pin(async move {
            let (_, remaining) = parse_common(&args);
            if remaining.is_empty() {
                // List all commands
                let commands = ShellCommands::list_commands();
                let _ = stdout.write_all(b"Available commands:\n").await;

                // Display in columns
                for chunk in commands.chunks(5) {
                    let line = chunk
                        .iter()
                        .map(|c| format!("{:<12}", c))
                        .collect::<Vec<_>>()
                        .join("");
                    let _ = stdout
                        .write_all(format!("  {}\n", line.trim_end()).as_bytes())
                        .await;
                }
                let _ = stdout
                    .write_all(
                        b"\nUse 'help COMMAND' for more information on a specific command.\n",
                    )
                    .await;
                0
            } else {
                // Show help for specific command
                let cmd_name = &remaining[0];
                if let Some(help) = ShellCommands::show_help(cmd_name) {
                    let _ = stdout.write_all(help.as_bytes()).await;
                    0
                } else {
                    let _ = stderr
                        .write_all(format!("help: no help for '{}'\n", cmd_name).as_bytes())
                        .await;
                    1
                }
            }
        })
    }
}

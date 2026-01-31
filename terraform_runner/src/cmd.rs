use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_generic_command(
    exec: &mut tokio::process::Command,
    max_output_lines: usize,
) -> Result<CommandResult, anyhow::Error> {
    let mut child = exec.spawn()?; // Start the command without waiting for it to finish
                                   // Check if `stdout` was successfully captured

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut last_stdout_lines = VecDeque::new();
    let mut last_stderr_lines = VecDeque::new();

    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            stdout_line = stdout_reader.next_line(), if !stdout_done => {
                match stdout_line {
                    Ok(Some(line)) => {
                        println!("{}", line); // Print each line to stdout
                        // Collect the line into the buffer
                        last_stdout_lines.push_back(line);
                        if last_stdout_lines.len() > max_output_lines {
                            last_stdout_lines.pop_front(); // Keep only the last N lines
                        }
                    },
                    Ok(None) => {
                        stdout_done = true; // EOF on stdout
                    },
                    Err(e) => {
                        eprintln!("Error reading stdout: {}", e);
                        stdout_done = true;
                    },
                }
            },
            stderr_line = stderr_reader.next_line(), if !stderr_done => {
                match stderr_line {
                    Ok(Some(line)) => {
                        // Collect the line into the buffer
                        last_stderr_lines.push_back(line);
                        if last_stderr_lines.len() > max_output_lines {
                            last_stderr_lines.pop_front(); // Keep only the last N lines
                        }
                    },
                    Ok(None) => {
                        stderr_done = true; // EOF on stderr
                    },
                    Err(e) => {
                        eprintln!("Error reading stderr: {}", e);
                        stderr_done = true;
                    },
                }
            },
        }
    }

    let exist_status = child.wait().await?;

    let stderr_text = last_stderr_lines
        .iter()
        .fold(String::new(), |acc, line| acc + line.as_str() + "\n");

    let stdout_text = last_stdout_lines
        .iter()
        .fold(String::new(), |acc, line| acc + line.as_str() + "\n");

    if !exist_status.success() {
        println!("Command failed with stderr:\n{}", stderr_text);
        if !stdout_text.is_empty() {
            println!("stdout:\n{}", stdout_text);
        }
        return Err(anyhow!("{}", stderr_text));
    }

    Ok(CommandResult {
        stdout: stdout_text,
        stderr: stderr_text,
    })
}

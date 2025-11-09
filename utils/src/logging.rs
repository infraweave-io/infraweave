use std::env;

use chrono::Local;
use log::LevelFilter;

pub fn setup_logging() -> Result<(), fern::InitError> {
    let base_config = fern::Dispatch::new();

    let level = match env::var("LOG_LEVEL").as_deref() {
        Ok("info") => LevelFilter::Info,
        Ok("debug") => LevelFilter::Debug,
        Ok("warn") => LevelFilter::Warn,
        Ok("error") => LevelFilter::Error,
        _ => LevelFilter::Warn, // Default to Warn if variable is unset or has an unrecognized value
    };

    let stderr_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}: {}",
                Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(level)
        .chain(std::io::stderr()); // Changed from stdout to stderr for MCP compatibility

    // let file_config = fern::Dispatch::new()
    //     .format(|out, message, record| {
    //         out.finish(format_args!(
    //             "{}[{}] {}: {}",
    //             Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
    //             record.target(),
    //             record.level(),
    //             message
    //         ))
    //     })
    //     .level(LevelFilter::Info)
    //     .chain(fern::log_file("output.log")?);

    base_config
        .chain(stderr_config)
        // .chain(file_config)
        .apply()?;

    Ok(())
}

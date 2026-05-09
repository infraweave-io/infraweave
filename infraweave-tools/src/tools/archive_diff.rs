use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::io::{Cursor, Read};
use zip::ZipArchive;

const MAX_RENDERED_FILES: usize = 20;
const MAX_DIFF_BYTES_PER_FILE: usize = 16 * 1024;
const MAX_FILE_BYTES: usize = 256 * 1024;

pub fn render_zip_diff(
    kind: &str,
    name: &str,
    track: &str,
    previous_version: &str,
    version: &str,
    old_zip: &[u8],
    new_zip: &[u8],
) -> Result<String> {
    let old_files = extract_zip_text_files(old_zip)
        .with_context(|| format!("could not read `{name}` {previous_version} zip"))?;
    let new_files = extract_zip_text_files(new_zip)
        .with_context(|| format!("could not read `{name}` {version} zip"))?;

    Ok(render_file_diff(
        kind,
        name,
        track,
        previous_version,
        version,
        &old_files,
        &new_files,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ZipFileContent {
    Text(String),
    Binary { bytes: usize },
    TooLarge { bytes: usize },
}

fn extract_zip_text_files(bytes: &[u8]) -> Result<BTreeMap<String, ZipFileContent>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("invalid zip archive")?;
    let mut files = BTreeMap::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }

        let name = file.name().trim_start_matches("./").to_string();
        if name.is_empty() {
            continue;
        }

        let size = file.size() as usize;
        if size > MAX_FILE_BYTES {
            files.insert(name, ZipFileContent::TooLarge { bytes: size });
            continue;
        }

        let mut content = Vec::with_capacity(size);
        file.read_to_end(&mut content)?;
        let content = match String::from_utf8(content) {
            Ok(text) if is_text_like(&text) => ZipFileContent::Text(text),
            Ok(text) => ZipFileContent::Binary { bytes: text.len() },
            Err(e) => ZipFileContent::Binary {
                bytes: e.into_bytes().len(),
            },
        };
        files.insert(name, content);
    }

    Ok(files)
}

fn render_file_diff(
    kind: &str,
    name: &str,
    track: &str,
    previous_version: &str,
    version: &str,
    old_files: &BTreeMap<String, ZipFileContent>,
    new_files: &BTreeMap<String, ZipFileContent>,
) -> String {
    let added: Vec<_> = new_files
        .keys()
        .filter(|path| !old_files.contains_key(*path))
        .cloned()
        .collect();
    let removed: Vec<_> = old_files
        .keys()
        .filter(|path| !new_files.contains_key(*path))
        .cloned()
        .collect();
    let changed: Vec<_> = old_files
        .iter()
        .filter_map(|(path, old)| {
            new_files
                .get(path)
                .filter(|new| *new != old)
                .map(|new| (path.clone(), old, new))
        })
        .collect();

    let mut out =
        format!("## Diff: {kind} `{name}` {previous_version} -> {version} (track: {track})\n\n");
    let n_add = added.len();
    let n_rm = removed.len();
    let n_ch = changed.len();
    let _ = writeln!(
        out,
        "**Summary:** {n_add} file(s) added, {n_rm} removed, {n_ch} changed.\n"
    );

    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        out.push_str("No file content changes found between the downloaded zips.");
        return out;
    }

    if !added.is_empty() {
        out.push_str("### Added files\n");
        for path in added.iter().take(MAX_RENDERED_FILES) {
            let content = &new_files[path];
            let _ = writeln!(out, "#### `{path}` ({})\n", describe_content(content));
            match content {
                ZipFileContent::Text(new_text) => {
                    render_text_diff(&mut out, "/dev/null", &format!("new/{path}"), "", new_text);
                }
                _ => {
                    out.push_str("- content omitted; not a text file that can be diffed inline\n");
                }
            }
            out.push('\n');
        }
        write_truncation_note(&mut out, added.len());
        out.push('\n');
    }

    if !removed.is_empty() {
        out.push_str("### Removed files\n");
        for path in removed.iter().take(MAX_RENDERED_FILES) {
            let content = &old_files[path];
            let _ = writeln!(out, "#### `{path}` ({})\n", describe_content(content));
            match content {
                ZipFileContent::Text(old_text) => {
                    render_text_diff(&mut out, &format!("old/{path}"), "/dev/null", old_text, "");
                }
                _ => {
                    out.push_str("- content omitted; not a text file that can be diffed inline\n");
                }
            }
            out.push('\n');
        }
        write_truncation_note(&mut out, removed.len());
        out.push('\n');
    }

    if !changed.is_empty() {
        out.push_str("### Changed files\n");
        for (path, old, new) in changed.iter().take(MAX_RENDERED_FILES) {
            let _ = writeln!(out, "#### `{path}`\n");
            match (old, new) {
                (ZipFileContent::Text(old_text), ZipFileContent::Text(new_text)) => {
                    render_text_diff(
                        &mut out,
                        &format!("old/{path}"),
                        &format!("new/{path}"),
                        old_text,
                        new_text,
                    );
                }
                _ => {
                    let _ = writeln!(
                        out,
                        "- changed from {} to {}",
                        describe_content(old),
                        describe_content(new)
                    );
                }
            }
            out.push('\n');
        }
        write_truncation_note(&mut out, changed.len());
    }
    out
}

fn render_text_diff(
    out: &mut String,
    old_filename: &str,
    new_filename: &str,
    old: &str,
    new: &str,
) {
    let mut options = diffy::DiffOptions::new();
    options
        .set_original_filename(old_filename.to_string())
        .set_modified_filename(new_filename.to_string());
    let patch = options.create_patch(old, new).to_string();
    let patch = truncate_diff(&patch);
    out.push_str("```diff\n");
    out.push_str(&patch);
    if !patch.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
}

fn truncate_diff(diff: &str) -> String {
    if diff.len() <= MAX_DIFF_BYTES_PER_FILE {
        return diff.to_string();
    }

    let mut truncated = String::with_capacity(MAX_DIFF_BYTES_PER_FILE + 32);
    for line in diff.lines() {
        if truncated.len() + line.len() + 1 > MAX_DIFF_BYTES_PER_FILE {
            break;
        }
        truncated.push_str(line);
        truncated.push('\n');
    }
    truncated.push_str("... diff truncated ...\n");
    truncated
}

fn describe_content(content: &ZipFileContent) -> String {
    match content {
        ZipFileContent::Text(text) => format!("text, {} bytes", text.len()),
        ZipFileContent::Binary { bytes } => format!("binary, {bytes} bytes"),
        ZipFileContent::TooLarge { bytes } => {
            format!("too large to diff inline, {bytes} bytes")
        }
    }
}

fn write_truncation_note(out: &mut String, count: usize) {
    if count > MAX_RENDERED_FILES {
        let omitted = count - MAX_RENDERED_FILES;
        let _ = writeln!(out, "- ... {omitted} more file(s) omitted");
    }
}

fn is_text_like(text: &str) -> bool {
    text.chars()
        .all(|c| c == '\n' || c == '\r' || c == '\t' || !c.is_control())
}

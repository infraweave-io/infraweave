/// Represents a file change with its path and content.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub content: String,
}

/// Contains two lists: one for active files (added/modified or renamed) and one for deleted files.
#[derive(Debug)]
pub struct ProcessedFiles {
    pub active_files: Vec<FileChange>,
    pub deleted_files: Vec<FileChange>,
}

/// A grouping key based on manifest identity.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct GroupKey {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub region: String,
}

/// Represents a grouped file with its manifest key and both before/after versions if available.
#[derive(Debug)]
pub struct GroupedFile {
    pub key: GroupKey,
    // The active file and its YAML document (single-document)
    pub active: Option<(FileChange, String)>,
    // The deleted file and its YAML document (if applicable)
    pub deleted: Option<(FileChange, String)>,
}

#[derive(Debug, Clone)]
pub struct ManifestChange {
    pub key: GroupKey,
    pub content: String, // YAML document representation of the manifest
    pub file: FileChange,
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Installation {
    pub id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Repository {
    pub owner: Owner,
    pub name: String,
    pub full_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Owner {
    pub login: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckRun {
    pub head_sha: String,
    pub status: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CheckRunOutput>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckRunOutput {
    pub title: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<CheckRunAnnotation>>,
}

// TODO: Could be used to highlight e.g. a typo in a file
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckRunAnnotation {
    pub path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub annotation_level: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JobDetails {
    pub region: String,
    pub environment: String,
    pub deployment_id: String,
    pub job_id: String,
    pub change_type: String,
    pub file_path: String,
    pub manifest_yaml: String,
    pub status: String,
    pub error_text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GitHubCheckRun {
    pub installation: Installation,
    pub app_id: String,
    pub repository: Repository,
    pub check_run: CheckRun,
    pub job_details: JobDetails,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)] // Used for trying to deserialize into one of the variants
pub enum ExtraData {
    GitHub(GitHubCheckRun),

    // This is a placeholder for future GitLab-specific events
    GitLab(GitLabCheckRun),
    // SEE: https://gitlab.com/-/user_settings/applications
    //      https://developers.cloudflare.com/pages/configuration/git-integration/gitlab-integration/
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct None {}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GitLabCheckRun {
    pub project: GitLabProject,
    pub pipeline: GitLabPipeline,
    pub job_details: JobDetails,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GitLabProject {
    pub id: i64,
    pub name: String,
    pub path_with_namespace: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GitLabPipeline {
    pub id: i64,
    pub sha: String,
    pub status: String,
    pub web_url: String,
    pub created_at: String,
    pub updated_at: String,
}

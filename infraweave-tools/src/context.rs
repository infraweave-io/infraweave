use crate::ApiClient;

/// Per-request context handed to every tool invocation.
///
/// `default_*` fields let the chat layer inject sensible defaults so the LLM
/// doesn't have to ask "what project?" on every turn - the user's session
/// already knows. Tools should still accept explicit overrides in their args.
#[derive(Clone)]
pub struct ToolContext {
    pub api: ApiClient,
    pub default_project: Option<String>,
    pub default_region: Option<String>,
    pub default_environment: Option<String>,
    pub default_track: Option<String>,
}

impl ToolContext {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api,
            default_project: None,
            default_region: None,
            default_environment: None,
            default_track: None,
        }
    }

    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.default_project = Some(project.into());
        self
    }

    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.default_region = Some(region.into());
        self
    }

    pub fn with_environment(mut self, environment: impl Into<String>) -> Self {
        self.default_environment = Some(environment.into());
        self
    }

    pub fn with_track(mut self, track: impl Into<String>) -> Self {
        self.default_track = Some(track.into());
        self
    }
}

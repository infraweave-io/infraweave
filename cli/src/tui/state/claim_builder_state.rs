use env_defs::{ModuleResp, TfVariable};
use env_utils::to_camel_case;

/// Represents a single variable input field in the claim builder form
#[derive(Debug, Clone)]
pub struct VariableInput {
    pub name: String,
    pub description: String,
    pub var_type: String,
    pub default_value: Option<String>,
    pub is_required: bool,
    pub is_sensitive: bool,
    pub user_value: String,
    pub cursor_position: usize,
}

impl VariableInput {
    pub fn from_tf_variable(var: &TfVariable) -> Self {
        let is_required = var.default.is_none();
        let default_str = var.default.as_ref().map(|v| {
            if v.is_null() {
                String::new()
            } else {
                serde_json::to_string(v).unwrap_or_default()
            }
        });

        Self {
            name: var.name.clone(),
            description: var.description.clone(),
            var_type: var._type.to_string(),
            default_value: default_str.clone(),
            is_required,
            is_sensitive: var.sensitive,
            // For non-required fields, start with empty string so default appears as placeholder
            user_value: if is_required {
                default_str.unwrap_or_default()
            } else {
                String::new()
            },
            cursor_position: 0,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        // Validate input based on type
        if !self.is_valid_char_for_type(c) {
            return;
        }

        self.user_value.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Check if a character is valid for this variable's type
    fn is_valid_char_for_type(&self, c: char) -> bool {
        let type_lower = self.var_type.to_lowercase();

        // For bool, only allow specific values
        if type_lower.contains("bool") {
            // Allow typing "true", "false", or empty
            let potential_value = format!(
                "{}{}{}",
                &self.user_value[..self.cursor_position],
                c,
                &self.user_value[self.cursor_position..]
            );

            // Check if it's a valid prefix of "true" or "false"
            return "true".starts_with(&potential_value.to_lowercase())
                || "false".starts_with(&potential_value.to_lowercase());
        }

        // For number types, only allow digits, minus, and decimal point
        if type_lower.contains("number") || type_lower.contains("int") {
            return c.is_numeric() || c == '-' || c == '.';
        }

        // For all other types, allow any character
        true
    }

    /// Validate the current value for this variable's type
    pub fn validate_value(&self) -> Result<(), String> {
        if self.user_value.is_empty() {
            if self.is_required {
                return Err(format!("Required field '{}' cannot be empty", self.name));
            }
            return Ok(());
        }

        let type_lower = self.var_type.to_lowercase();

        // Bool validation
        if type_lower.contains("bool") {
            let val = self.user_value.to_lowercase();
            if val != "true" && val != "false" {
                return Err(format!(
                    "Bool field must be 'true' or 'false', got '{}'",
                    self.user_value
                ));
            }
        }

        // Number validation
        if type_lower.contains("number") || type_lower.contains("int") {
            if self.user_value.parse::<f64>().is_err() {
                return Err(format!(
                    "Number field must be numeric, got '{}'",
                    self.user_value
                ));
            }
        }

        // Map/Object validation - check if it's valid JSON
        if type_lower.contains("map") || type_lower.contains("object") {
            if !self.user_value.starts_with('{') {
                return Err(format!(
                    "Map field must be a JSON object starting with '{{'"
                ));
            }
            if serde_json::from_str::<serde_json::Value>(&self.user_value).is_err() {
                return Err(format!("Map field must be valid JSON"));
            }
        }

        // List/Array validation
        if type_lower.contains("list") || type_lower.contains("array") || type_lower.contains("set")
        {
            if !self.user_value.starts_with('[') {
                return Err(format!("List field must be a JSON array starting with '['"));
            }
            if serde_json::from_str::<serde_json::Value>(&self.user_value).is_err() {
                return Err(format!("List field must be valid JSON"));
            }
        }

        Ok(())
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            // For bool types, prevent deletion that would create invalid state
            let type_lower = self.var_type.to_lowercase();
            if type_lower.contains("bool") && !self.user_value.is_empty() {
                let mut test_value = self.user_value.clone();
                test_value.remove(self.cursor_position - 1);

                // Only allow deletion if result is empty or valid prefix
                if !test_value.is_empty() {
                    let test_lower = test_value.to_lowercase();
                    if !("true".starts_with(&test_lower) || "false".starts_with(&test_lower)) {
                        // Don't allow this deletion
                        return;
                    }
                }
            }

            self.user_value.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.user_value.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.user_value.len();
    }
}

/// State for the claim builder view
#[derive(Debug, Clone)]
pub struct ClaimBuilderState {
    pub showing_claim_builder: bool,
    pub source_module: Option<ModuleResp>,
    pub source_stack: Option<ModuleResp>,
    pub is_stack: bool,

    // Form fields
    pub deployment_name: String,
    pub deployment_name_cursor: usize,
    pub region: String,
    pub region_cursor: usize,

    // Variable inputs
    pub variable_inputs: Vec<VariableInput>,

    // Navigation
    pub selected_field_index: usize, // 0: name, 1: region, 2+: variables
    pub scroll_offset: u16,

    // Generated YAML
    pub generated_yaml: String,
    pub show_preview: bool,
    pub preview_scroll: u16,

    // Validation
    pub validation_error: Option<String>,
}

impl ClaimBuilderState {
    pub fn new() -> Self {
        Self {
            showing_claim_builder: false,
            source_module: None,
            source_stack: None,
            is_stack: false,
            deployment_name: String::new(),
            deployment_name_cursor: 0,
            region: String::from(""), // Default region
            region_cursor: 0,         // Cursor at end of default value
            variable_inputs: Vec::new(),
            selected_field_index: 0,
            scroll_offset: 0,
            generated_yaml: String::new(),
            show_preview: false,
            preview_scroll: 0,
            validation_error: None,
        }
    }

    /// Open the claim builder for a module
    pub fn open_for_module(&mut self, module: ModuleResp) {
        self.source_module = Some(module.clone());
        self.source_stack = None;
        self.is_stack = false;
        self.showing_claim_builder = true;
        self.initialize_from_module(&module);
    }

    /// Open the claim builder for a stack
    pub fn open_for_stack(&mut self, stack: ModuleResp) {
        self.source_module = None;
        self.source_stack = Some(stack.clone());
        self.is_stack = true;
        self.showing_claim_builder = true;
        self.initialize_from_module(&stack);
    }

    fn initialize_from_module(&mut self, module: &ModuleResp) {
        // Initialize form fields
        self.deployment_name = String::new();
        self.deployment_name_cursor = 0;
        self.region = String::from(""); // Reset to default
        self.region_cursor = 0; // Cursor at end of default value

        // Initialize variable inputs from module's tf_variables
        let mut variables: Vec<_> = module
            .tf_variables
            .iter()
            .map(VariableInput::from_tf_variable)
            .collect();

        // For stacks, keep variables grouped by module instance (don't sort by required)
        // For modules, sort required fields first
        if !self.is_stack {
            variables.sort_by(|a, b| match (a.is_required, b.is_required) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });
        }
        // For stacks, variables are already in the correct order from tf_variables
        // (grouped by instance like bucket1a__*, bucket2__*)

        self.variable_inputs = variables;

        self.selected_field_index = 0;
        self.scroll_offset = 0;
        self.show_preview = false;
        self.preview_scroll = 0;
    }

    /// Determines if a variable should be included in the generated YAML
    /// Only include variables that:
    /// 1. Are required (no default value), OR
    /// 2. Have been explicitly changed from their default value
    fn should_include_variable(var: &VariableInput) -> bool {
        // Always include required variables (even if empty, will be caught in validation)
        if var.is_required {
            return true;
        }

        // Skip empty non-required variables
        if var.user_value.is_empty() {
            return false;
        }

        // If there's no default, include it (shouldn't happen for non-required, but defensive)
        let Some(default_value) = &var.default_value else {
            return true;
        };

        // Normalize both values for comparison
        let user_normalized = Self::normalize_value(&var.user_value);
        let default_normalized = Self::normalize_value(default_value);

        // Include only if the value differs from the default
        user_normalized != default_normalized
    }

    /// Normalize a value string for comparison
    /// Parses as JSON if possible, otherwise compares as string
    fn normalize_value(value: &str) -> String {
        // Empty values normalize to empty string
        if value.is_empty() {
            return String::new();
        }

        // Try to parse as JSON for semantic comparison
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
            // Serialize back in canonical form (no whitespace variations)
            serde_json::to_string(&parsed).unwrap_or_else(|_| value.to_string())
        } else {
            // Not valid JSON, compare as-is
            value.trim().to_string()
        }
    }

    /// Close the claim builder
    pub fn close(&mut self) {
        self.showing_claim_builder = false;
        self.source_module = None;
        self.source_stack = None;
        self.variable_inputs.clear();
        self.deployment_name.clear();
        self.generated_yaml.clear();
        self.selected_field_index = 0;
        self.scroll_offset = 0;
        self.show_preview = false;
        self.preview_scroll = 0;
    }

    /// Get the total number of fields (2 base fields + variables)
    pub fn total_fields(&self) -> usize {
        2 + self.variable_inputs.len()
    }

    /// Move to the next field
    pub fn next_field(&mut self) {
        // Validate and auto-correct current field before moving
        self.autocorrect_field();

        if self.selected_field_index < self.total_fields() - 1 {
            self.selected_field_index += 1;
        }
    }

    /// Move to the previous field
    pub fn previous_field(&mut self) {
        // Validate and auto-correct current field before moving
        self.autocorrect_field();

        if self.selected_field_index > 0 {
            self.selected_field_index -= 1;
        }
    }

    /// Auto-correct the current field value based on its type
    fn autocorrect_field(&mut self) {
        if self.selected_field_index == 0 || self.selected_field_index == 1 {
            return; // Deployment name and region don't need autocorrection
        }

        let var_index = self.selected_field_index - 2;
        if let Some(input) = self.variable_inputs.get_mut(var_index) {
            let type_lower = input.var_type.to_lowercase();

            // For bool fields, auto-complete or clear invalid values
            if type_lower.contains("bool") && !input.user_value.is_empty() {
                let val_lower = input.user_value.to_lowercase();

                // Auto-complete partial values
                if "true".starts_with(&val_lower) {
                    input.user_value = "true".to_string();
                    input.cursor_position = 4;
                } else if "false".starts_with(&val_lower) {
                    input.user_value = "false".to_string();
                    input.cursor_position = 5;
                } else {
                    // Invalid - clear it
                    input.user_value.clear();
                    input.cursor_position = 0;
                }
            }
        }
    }

    /// Insert a character at the current field's cursor position
    pub fn insert_char(&mut self, c: char) {
        // Clear validation error when user starts typing
        self.validation_error = None;

        match self.selected_field_index {
            0 => {
                self.deployment_name.insert(self.deployment_name_cursor, c);
                self.deployment_name_cursor += 1;
            }
            1 => {
                self.region.insert(self.region_cursor, c);
                self.region_cursor += 1;
            }
            i if i >= 2 => {
                let var_index = i - 2;
                if let Some(input) = self.variable_inputs.get_mut(var_index) {
                    input.insert_char(c);
                }
            }
            _ => {}
        }
    }

    /// Delete the character before the cursor
    pub fn backspace(&mut self) {
        // Clear validation error when user starts editing
        self.validation_error = None;

        match self.selected_field_index {
            0 => {
                if self.deployment_name_cursor > 0 {
                    self.deployment_name.remove(self.deployment_name_cursor - 1);
                    self.deployment_name_cursor -= 1;
                }
            }
            1 => {
                if self.region_cursor > 0 {
                    self.region.remove(self.region_cursor - 1);
                    self.region_cursor -= 1;
                }
            }
            i if i >= 2 => {
                let var_index = i - 2;
                if let Some(input) = self.variable_inputs.get_mut(var_index) {
                    input.delete_char();
                }
            }
            _ => {}
        }
    }

    /// Move cursor left in the current field
    pub fn move_cursor_left(&mut self) {
        match self.selected_field_index {
            0 => {
                if self.deployment_name_cursor > 0 {
                    self.deployment_name_cursor -= 1;
                }
            }
            1 => {
                if self.region_cursor > 0 {
                    self.region_cursor -= 1;
                }
            }
            i if i >= 2 => {
                let var_index = i - 2;
                if let Some(input) = self.variable_inputs.get_mut(var_index) {
                    input.move_cursor_left();
                }
            }
            _ => {}
        }
    }

    /// Move cursor right in the current field
    pub fn move_cursor_right(&mut self) {
        match self.selected_field_index {
            0 => {
                if self.deployment_name_cursor < self.deployment_name.len() {
                    self.deployment_name_cursor += 1;
                }
            }
            1 => {
                if self.region_cursor < self.region.len() {
                    self.region_cursor += 1;
                }
            }
            i if i >= 2 => {
                let var_index = i - 2;
                if let Some(input) = self.variable_inputs.get_mut(var_index) {
                    input.move_cursor_right();
                }
            }
            _ => {}
        }
    }

    /// Insert a template/default value for the current field based on its type
    pub fn insert_template(&mut self) {
        if self.selected_field_index == 0 || self.selected_field_index == 1 {
            return; // No template for deployment name or region
        }

        let var_index = self.selected_field_index - 2;
        if let Some(input) = self.variable_inputs.get_mut(var_index) {
            let type_lower = input.var_type.to_lowercase();

            let template = if type_lower.contains("bool") {
                "false".to_string()
            } else if type_lower.contains("map") || type_lower.contains("object") {
                "{}".to_string()
            } else if type_lower.contains("list")
                || type_lower.contains("array")
                || type_lower.contains("set")
            {
                "[]".to_string()
            } else if type_lower.contains("number") || type_lower.contains("int") {
                "0".to_string()
            } else {
                // String type - no template needed
                return;
            };

            // Clear current value and insert template
            input.user_value = template.clone();
            input.cursor_position = template.len();
        }
    }

    /// Toggle preview mode
    pub fn toggle_preview(&mut self) {
        // If trying to show preview, validate first
        if !self.show_preview {
            // Validate all fields
            if let Err(err) = self.validate_all_fields() {
                self.validation_error = Some(err);
                return; // Don't toggle to preview if validation fails
            }
        }

        // Clear any previous validation errors
        self.validation_error = None;

        self.show_preview = !self.show_preview;
        if self.show_preview {
            self.generate_yaml();
        }
    }

    /// Validate all fields in the form
    fn validate_all_fields(&self) -> Result<(), String> {
        // Validate deployment name
        if self.deployment_name.is_empty() {
            return Err("Deployment name is required".to_string());
        }

        // Validate region
        if self.region.is_empty() {
            return Err("Region is required".to_string());
        }

        // Validate all variable inputs
        for var in &self.variable_inputs {
            if let Err(err) = var.validate_value() {
                return Err(err);
            }
        }

        Ok(())
    }

    /// Generate the deployment claim YAML using the existing utility function
    pub fn generate_yaml(&mut self) {
        let module_ref = if self.is_stack {
            self.source_stack.as_ref()
        } else {
            self.source_module.as_ref()
        };

        let Some(module) = module_ref else {
            self.generated_yaml = "Error: No module or stack loaded".to_string();
            return;
        };

        // Build variables map from user inputs
        let variables = if self.is_stack {
            // For stacks, variables are named like "bucket1a__bucket_name"
            // We need to unflatten them into nested objects like:
            // bucket1a:
            //   bucketName: value
            let mut stack_vars: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

            for var in &self.variable_inputs {
                // Skip variables that haven't been explicitly set or differ from default
                if !Self::should_include_variable(var) {
                    continue;
                }

                // Split on double underscore to get module instance and variable name
                if let Some((instance_name, var_name)) = var.name.split_once("__") {
                    // Get or create the module instance object
                    let instance_obj = stack_vars
                        .entry(instance_name.to_string())
                        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

                    if let Some(obj) = instance_obj.as_object_mut() {
                        // Convert snake_case to camelCase for the variable name
                        let camel_name = to_camel_case(var_name);

                        // Parse the value
                        let value = if var.user_value.contains("{{") {
                            serde_json::Value::String(var.user_value.clone())
                        } else if let Ok(parsed) =
                            serde_json::from_str::<serde_json::Value>(&var.user_value)
                        {
                            parsed
                        } else {
                            serde_json::Value::String(var.user_value.clone())
                        };
                        obj.insert(camel_name, value);
                    }
                }
            }

            serde_json::Value::Object(stack_vars)
        } else {
            // For modules, keep as snake_case - generate_deployment_claim will handle conversion
            let mut module_vars = serde_json::Map::new();
            for var in &self.variable_inputs {
                // Skip variables that haven't been explicitly set or differ from default
                if !Self::should_include_variable(var) {
                    continue;
                }

                let value = if var.user_value.contains("{{") {
                    serde_json::Value::String(var.user_value.clone())
                } else if let Ok(parsed) =
                    serde_json::from_str::<serde_json::Value>(&var.user_value)
                {
                    parsed
                } else {
                    serde_json::Value::String(var.user_value.clone())
                };
                module_vars.insert(var.name.clone(), value);
            }
            serde_json::Value::Object(module_vars)
        };

        // Create a minimal DeploymentResp for use with generate_deployment_claim
        let deployment = env_defs::DeploymentResp {
            deployment_id: format!("default/{}", &self.deployment_name),
            environment: "default".to_string(),
            region: self.region.clone(), // Use the user-provided region
            module_type: if self.is_stack { "stack" } else { "module" }.to_string(),
            variables,
            // Fill in other required fields with defaults
            epoch: 0,
            status: String::new(),
            job_id: String::new(),
            project_id: String::new(),
            module: module.module_name.clone(),
            module_version: module.version.clone(),
            module_track: String::new(),
            drift_detection: env_defs::DriftDetection {
                enabled: false,
                interval: "24h".to_string(),
                auto_remediate: false,
                webhooks: Vec::new(),
            },
            next_drift_check_epoch: 0,
            has_drifted: false,
            output: serde_json::Value::Null,
            policy_results: Vec::new(),
            error_text: String::new(),
            deleted: false,
            dependencies: Vec::new(),
            initiated_by: String::new(),
            cpu: String::new(),
            memory: String::new(),
            reference: String::new(),
            tf_resources: None,
        };

        // Use the existing generate_deployment_claim function
        self.generated_yaml = env_utils::generate_deployment_claim(&deployment, module);
    }

    /// Scroll preview up
    pub fn scroll_preview_up(&mut self) {
        if self.preview_scroll > 0 {
            self.preview_scroll -= 1;
        }
    }

    /// Scroll preview down
    pub fn scroll_preview_down(&mut self) {
        self.preview_scroll += 1;
    }

    /// Validate the form (public method for app.rs)
    pub fn validate(&self) -> Result<(), String> {
        self.validate_all_fields()
    }

    /// Get the current cursor position for the selected field
    pub fn get_current_cursor_position(&self) -> usize {
        match self.selected_field_index {
            0 => self.deployment_name_cursor,
            1 => self.region_cursor,
            i if i >= 2 => {
                let var_index = i - 2;
                self.variable_inputs
                    .get(var_index)
                    .map(|v| v.cursor_position)
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }
}

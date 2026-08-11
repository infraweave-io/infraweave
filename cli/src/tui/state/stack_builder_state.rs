use env_defs::{ModuleResp, TfVariable};
use env_utils::to_camel_case;

/// Represents a module instance in the stack being built
#[derive(Debug, Clone)]
pub struct ModuleInstance {
    pub instance_name: String,
    pub module: Option<ModuleResp>,
    pub module_name: String,
    pub version: String,
    pub variable_inputs: Vec<VariableInput>,
}

/// Represents a single variable input field
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
            user_value: if is_required {
                default_str.unwrap_or_default()
            } else {
                String::new()
            },
            cursor_position: 0,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.user_value.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
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
}

/// The different pages/steps in the stack builder workflow
#[derive(Debug, Clone, PartialEq)]
pub enum StackBuilderPage {
    ModuleList,            // Page showing list of added modules
    VariableConfiguration, // Page to configure variables for each module instance
    Preview,               // Page to preview the generated YAML
}

/// State for the stack builder view
#[derive(Debug, Clone)]
pub struct StackBuilderState {
    pub showing_stack_builder: bool,
    pub current_page: StackBuilderPage,

    // Stack metadata
    pub stack_name: String,
    pub stack_name_cursor: usize,
    pub editing_stack_name: bool,

    // Module selection modal state
    pub showing_module_modal: bool,
    pub available_modules: Vec<ModuleResp>,
    pub modal_selected_index: usize,
    pub modal_scroll_offset: u16,

    // Module instances
    pub module_instances: Vec<ModuleInstance>,
    pub selected_instance_index: usize,

    // Instance name input (when adding a module)
    pub instance_name_input: String,
    pub instance_name_cursor: usize,
    pub editing_instance_name: bool,

    // Variable configuration state
    pub current_instance_index: usize,
    pub selected_variable_index: usize,
    pub scroll_offset: u16,

    // Reference picker modal (for cross-module references)
    pub showing_reference_picker: bool,
    pub reference_picker_step: ReferencePickerStep, // 0 = select instance, 1 = select output
    pub reference_selected_instance_index: usize,
    pub reference_selected_output_index: usize,
    pub reference_picker_scroll_offset: u16,

    // Preview state
    pub generated_yaml: String,
    pub preview_scroll: u16,

    // Individual YAML files (filename, content)
    pub generated_files: Vec<(String, String)>,

    // Validation
    pub validation_error: Option<String>,
}

/// Step in the reference picker workflow
#[derive(Debug, Clone, PartialEq)]
pub enum ReferencePickerStep {
    SelectInstance,
    SelectOutput,
}

impl StackBuilderState {
    pub fn new() -> Self {
        Self {
            showing_stack_builder: false,
            current_page: StackBuilderPage::ModuleList,
            stack_name: String::new(),
            stack_name_cursor: 0,
            editing_stack_name: false,
            showing_module_modal: false,
            available_modules: Vec::new(),
            modal_selected_index: 0,
            modal_scroll_offset: 0,
            module_instances: Vec::new(),
            selected_instance_index: 0,
            instance_name_input: String::new(),
            instance_name_cursor: 0,
            editing_instance_name: false,
            current_instance_index: 0,
            selected_variable_index: 0,
            scroll_offset: 0,
            showing_reference_picker: false,
            reference_picker_step: ReferencePickerStep::SelectInstance,
            reference_selected_instance_index: 0,
            reference_selected_output_index: 0,
            reference_picker_scroll_offset: 0,
            generated_yaml: String::new(),
            preview_scroll: 0,
            generated_files: Vec::new(),
            validation_error: None,
        }
    }

    /// Open the stack builder
    pub fn open(&mut self, available_modules: Vec<ModuleResp>) {
        self.showing_stack_builder = true;
        self.current_page = StackBuilderPage::ModuleList;
        self.available_modules = available_modules;
        self.module_instances.clear();
        self.stack_name.clear();
        self.stack_name_cursor = 0;
        self.editing_stack_name = true; // Start by editing stack name
        self.showing_module_modal = false;
        self.modal_selected_index = 0;
        self.instance_name_input.clear();
        self.instance_name_cursor = 0;
        self.validation_error = None;
    }

    /// Close the stack builder
    pub fn close(&mut self) {
        self.showing_stack_builder = false;
        self.module_instances.clear();
        self.stack_name.clear();
        self.generated_yaml.clear();
        self.current_page = StackBuilderPage::ModuleList;
        self.showing_module_modal = false;
        self.editing_stack_name = false;
        self.editing_instance_name = false;
    }

    /// Open the module selection modal
    pub fn open_module_modal(&mut self) {
        self.showing_module_modal = true;
        self.modal_selected_index = 0;
        self.modal_scroll_offset = 0;
        self.instance_name_input.clear();
        self.instance_name_cursor = 0;
        self.editing_instance_name = false;
        self.editing_stack_name = false; // Make sure we're not editing stack name
    }

    /// Close the module selection modal
    pub fn close_module_modal(&mut self) {
        self.showing_module_modal = false;
        self.instance_name_input.clear();
        self.editing_instance_name = false;
    }

    /// Move to next module in modal
    pub fn next_modal_module(&mut self) {
        if self.modal_selected_index < self.available_modules.len().saturating_sub(1) {
            self.modal_selected_index += 1;
        }
    }

    /// Move to previous module in modal
    pub fn previous_modal_module(&mut self) {
        if self.modal_selected_index > 0 {
            self.modal_selected_index -= 1;
        }
    }

    /// Jump down by page_size in modal (Page Down)
    pub fn page_down_modal(&mut self, page_size: usize) {
        let max_index = self.available_modules.len().saturating_sub(1);
        self.modal_selected_index = (self.modal_selected_index + page_size).min(max_index);
    }

    /// Jump up by page_size in modal (Page Up)
    pub fn page_up_modal(&mut self, page_size: usize) {
        self.modal_selected_index = self.modal_selected_index.saturating_sub(page_size);
    }

    /// Update scroll offset to ensure selected item is visible
    /// Should be called during rendering when visible_height is known
    pub fn update_modal_scroll(&mut self, visible_height: usize) {
        let scroll_offset = self.modal_scroll_offset as usize;

        // If selected item is below visible area, scroll down
        if self.modal_selected_index >= scroll_offset + visible_height {
            self.modal_scroll_offset = (self.modal_selected_index - visible_height + 1) as u16;
        }
        // If selected item is above visible area, scroll up
        else if self.modal_selected_index < scroll_offset {
            self.modal_scroll_offset = self.modal_selected_index as u16;
        }
    }

    /// Select the current module and prompt for instance name
    pub fn select_modal_module(&mut self) {
        self.editing_instance_name = true;
        // Default to module name as instance name (lowercase)
        if let Some(module) = self.available_modules.get(self.modal_selected_index) {
            self.instance_name_input = module.module_name.to_lowercase();
            self.instance_name_cursor = self.instance_name_input.len();
        } else {
            self.instance_name_input.clear();
            self.instance_name_cursor = 0;
        }
    }

    /// Add a new module instance to the stack
    pub fn add_module_instance(&mut self) -> Result<(), String> {
        // Validate inputs
        if self.instance_name_input.trim().is_empty() {
            return Err("Instance name cannot be empty".to_string());
        }

        // Convert instance name to lowercase
        let instance_name = self.instance_name_input.trim().to_lowercase();

        // Check for duplicate instance names
        if self
            .module_instances
            .iter()
            .any(|m| m.instance_name == instance_name)
        {
            return Err(format!("Instance name '{}' already exists", instance_name));
        }

        if self.modal_selected_index >= self.available_modules.len() {
            return Err("Invalid module selection".to_string());
        }

        let selected_module = &self.available_modules[self.modal_selected_index];

        // Create variable inputs for this module instance
        let variable_inputs: Vec<_> = selected_module
            .tf_variables
            .iter()
            .map(VariableInput::from_tf_variable)
            .collect();

        let instance = ModuleInstance {
            instance_name,
            module: Some(selected_module.clone()),
            module_name: selected_module.module_name.clone(),
            version: selected_module.version.clone(), // Use the selected module's version
            variable_inputs,
        };

        self.module_instances.push(instance);

        // Close the modal and clear inputs
        self.close_module_modal();

        Ok(())
    }

    /// Remove a module instance
    pub fn remove_module_instance(&mut self, index: usize) {
        if index < self.module_instances.len() {
            self.module_instances.remove(index);
        }
    }

    /// Move to the next page
    pub fn next_page(&mut self) -> Result<(), String> {
        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.module_instances.is_empty() {
                    return Err("Please add at least one module instance".to_string());
                }
                if self.stack_name.trim().is_empty() {
                    return Err("Stack name cannot be empty".to_string());
                }

                // Ensure stack name starts with a capital letter
                let trimmed = self.stack_name.trim().to_string();
                if !trimmed.is_empty() {
                    let mut chars = trimmed.chars();
                    if let Some(first_char) = chars.next() {
                        self.stack_name =
                            first_char.to_uppercase().collect::<String>() + chars.as_str();
                    }
                }

                self.current_page = StackBuilderPage::VariableConfiguration;
                self.current_instance_index = 0;
                self.selected_variable_index = 0;
            }
            StackBuilderPage::VariableConfiguration => {
                // Check that all required variables are set across all instances
                let mut missing_vars = Vec::new();

                for (idx, instance) in self.module_instances.iter().enumerate() {
                    for var in &instance.variable_inputs {
                        if var.is_required
                            && var.user_value.trim().is_empty()
                            && var.default_value.is_none()
                        {
                            missing_vars.push(format!("{}.{}", instance.instance_name, var.name));
                        }
                    }
                }

                if !missing_vars.is_empty() {
                    return Err(format!(
                        "Required variables not set: {}",
                        missing_vars.join(", ")
                    ));
                }

                self.generate_yaml();
                self.current_page = StackBuilderPage::Preview;
            }
            StackBuilderPage::Preview => {
                // Already on last page
            }
        }
        Ok(())
    }

    /// Move to the previous page
    pub fn previous_page(&mut self) {
        match self.current_page {
            StackBuilderPage::ModuleList => {
                // Already on first page
            }
            StackBuilderPage::VariableConfiguration => {
                self.current_page = StackBuilderPage::ModuleList;
            }
            StackBuilderPage::Preview => {
                self.current_page = StackBuilderPage::VariableConfiguration;
            }
        }
    }

    /// Move to the next module instance in variable configuration
    pub fn next_instance(&mut self) {
        if self.current_instance_index < self.module_instances.len().saturating_sub(1) {
            self.current_instance_index += 1;
            self.selected_variable_index = 0;
            self.scroll_offset = 0;
        }
    }

    /// Move to the previous module instance in variable configuration
    pub fn previous_instance(&mut self) {
        if self.current_instance_index > 0 {
            self.current_instance_index -= 1;
            self.selected_variable_index = 0;
            self.scroll_offset = 0;
        }
    }

    /// Move to the next module instance in the module list (for selection/deletion)
    pub fn next_selected_instance(&mut self) {
        if self.selected_instance_index < self.module_instances.len().saturating_sub(1) {
            self.selected_instance_index += 1;
        }
    }

    /// Move to the previous module instance in the module list (for selection/deletion)
    pub fn previous_selected_instance(&mut self) {
        if self.selected_instance_index > 0 {
            self.selected_instance_index -= 1;
        }
    }

    /// Move to the next variable field
    pub fn next_variable(&mut self) {
        if let Some(instance) = self.module_instances.get(self.current_instance_index) {
            if self.selected_variable_index < instance.variable_inputs.len().saturating_sub(1) {
                self.selected_variable_index += 1;
            }
        }
    }

    /// Move to the previous variable field
    pub fn previous_variable(&mut self) {
        if self.selected_variable_index > 0 {
            self.selected_variable_index -= 1;
        }
    }

    /// Insert a character at the current field's cursor position
    pub fn insert_char(&mut self, c: char) {
        self.validation_error = None;

        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.editing_stack_name {
                    self.stack_name.insert(self.stack_name_cursor, c);
                    self.stack_name_cursor += 1;
                } else if self.editing_instance_name {
                    self.instance_name_input
                        .insert(self.instance_name_cursor, c);
                    self.instance_name_cursor += 1;
                }
            }
            StackBuilderPage::VariableConfiguration => {
                if let Some(instance) = self.module_instances.get_mut(self.current_instance_index) {
                    if let Some(var) = instance
                        .variable_inputs
                        .get_mut(self.selected_variable_index)
                    {
                        var.insert_char(c);
                    }
                }
            }
            StackBuilderPage::Preview => {}
        }
    }

    /// Delete the character before the cursor
    pub fn backspace(&mut self) {
        self.validation_error = None;

        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.editing_stack_name {
                    if self.stack_name_cursor > 0 {
                        self.stack_name.remove(self.stack_name_cursor - 1);
                        self.stack_name_cursor -= 1;
                    }
                } else if self.editing_instance_name {
                    if self.instance_name_cursor > 0 {
                        self.instance_name_input
                            .remove(self.instance_name_cursor - 1);
                        self.instance_name_cursor -= 1;
                    }
                }
            }
            StackBuilderPage::VariableConfiguration => {
                if let Some(instance) = self.module_instances.get_mut(self.current_instance_index) {
                    if let Some(var) = instance
                        .variable_inputs
                        .get_mut(self.selected_variable_index)
                    {
                        var.backspace();
                    }
                }
            }
            StackBuilderPage::Preview => {}
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.editing_stack_name {
                    if self.stack_name_cursor > 0 {
                        self.stack_name_cursor -= 1;
                    }
                } else if self.editing_instance_name {
                    if self.instance_name_cursor > 0 {
                        self.instance_name_cursor -= 1;
                    }
                }
            }
            StackBuilderPage::VariableConfiguration => {
                if let Some(instance) = self.module_instances.get_mut(self.current_instance_index) {
                    if let Some(var) = instance
                        .variable_inputs
                        .get_mut(self.selected_variable_index)
                    {
                        var.move_cursor_left();
                    }
                }
            }
            StackBuilderPage::Preview => {}
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.editing_stack_name {
                    if self.stack_name_cursor < self.stack_name.len() {
                        self.stack_name_cursor += 1;
                    }
                } else if self.editing_instance_name {
                    if self.instance_name_cursor < self.instance_name_input.len() {
                        self.instance_name_cursor += 1;
                    }
                }
            }
            StackBuilderPage::VariableConfiguration => {
                if let Some(instance) = self.module_instances.get_mut(self.current_instance_index) {
                    if let Some(var) = instance
                        .variable_inputs
                        .get_mut(self.selected_variable_index)
                    {
                        var.move_cursor_right();
                    }
                }
            }
            StackBuilderPage::Preview => {}
        }
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

    /// Page up in preview (jump by page_size lines)
    pub fn page_up_preview(&mut self, page_size: u16) {
        self.preview_scroll = self.preview_scroll.saturating_sub(page_size);
    }

    /// Page down in preview (jump by page_size lines)
    pub fn page_down_preview(&mut self, page_size: u16) {
        self.preview_scroll = self.preview_scroll.saturating_add(page_size);
    }

    /// Generate the stack deployment YAML
    pub fn generate_yaml(&mut self) {
        let mut yaml_parts = Vec::new();
        self.generated_files.clear();

        // Add Stack definition at the beginning
        let stack_definition = format!(
            "apiVersion: infraweave.io/v1
kind: Stack
metadata:
  name: {}
spec:
  stackName: {}
  version: 0.1.0
  reference: https://github.com/your-org/{}
  description: |
    Stack containing {} module(s).",
            self.stack_name.to_lowercase().replace(" ", "-"),
            self.stack_name,
            self.stack_name.to_lowercase().replace(" ", "-"),
            self.module_instances.len()
        );
        yaml_parts.push(stack_definition.clone());

        // Save Stack definition as stack.yaml
        self.generated_files
            .push(("stack.yaml".to_string(), stack_definition));

        for instance in &self.module_instances {
            let module_name = &instance.module_name;

            // Build variables map
            let mut variables_map = serde_json::Map::new();
            for var in &instance.variable_inputs {
                // Skip variables that haven't been set and aren't required
                if var.user_value.is_empty() && !var.is_required {
                    continue;
                }

                let value = if var.user_value.is_empty() {
                    // Use default value
                    if let Some(default) = &var.default_value {
                        default.clone()
                    } else {
                        continue;
                    }
                } else {
                    var.user_value.clone()
                };

                // Convert snake_case to camelCase
                let camel_name = to_camel_case(&var.name);

                // Parse value if it's JSON, otherwise keep as string
                let parsed_value = if value.contains("{{") {
                    // It's a template reference, keep as string
                    serde_json::Value::String(value)
                } else if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value) {
                    parsed
                } else {
                    serde_json::Value::String(value)
                };

                variables_map.insert(camel_name, parsed_value);
            }

            // Create the deployment claim YAML for this instance
            let mut claim = String::new();
            claim.push_str(&format!("apiVersion: infraweave.io/v1\n"));
            claim.push_str(&format!("kind: {}\n", module_name));
            claim.push_str("metadata:\n");
            claim.push_str(&format!("  name: {}\n", instance.instance_name));
            claim.push_str("spec:\n");
            claim.push_str(&format!("  moduleVersion: {}\n", instance.version));
            claim.push_str("  region: N/A\n");

            if !variables_map.is_empty() {
                claim.push_str("  variables:\n");

                // Convert the variables map to YAML manually to ensure proper formatting
                for (key, value) in variables_map {
                    let value_str = match value {
                        serde_json::Value::String(s) => {
                            if s.contains("{{") {
                                // Template reference - must be quoted
                                format!("\"{}\"", s)
                            } else {
                                s
                            }
                        }
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Null => "null".to_string(),
                        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                            serde_json::to_string(&value).unwrap_or_default()
                        }
                    };

                    claim.push_str(&format!("    {}: {}\n", key, value_str));
                }
            }

            yaml_parts.push(claim.clone());

            // Save claim as separate file using instance name
            let claim_filename = format!("{}.yaml", instance.instance_name);
            self.generated_files.push((claim_filename, claim));
        }

        self.generated_yaml = yaml_parts.join("\n---\n\n");
    }

    /// Get the current cursor position for the active field
    pub fn get_current_cursor_position(&self) -> usize {
        match self.current_page {
            StackBuilderPage::ModuleList => {
                if self.editing_stack_name {
                    self.stack_name_cursor
                } else if self.editing_instance_name {
                    self.instance_name_cursor
                } else {
                    0
                }
            }
            StackBuilderPage::VariableConfiguration => {
                if let Some(instance) = self.module_instances.get(self.current_instance_index) {
                    if let Some(var) = instance.variable_inputs.get(self.selected_variable_index) {
                        return var.cursor_position;
                    }
                }
                0
            }
            StackBuilderPage::Preview => 0,
        }
    }

    // Modal control helpers
    pub fn cancel_modal(&mut self) {
        self.close_module_modal();
    }

    pub fn confirm_modal_selection(&mut self) -> Result<(), String> {
        self.add_module_instance()?;
        self.close_module_modal();
        Ok(())
    }

    // Reference picker methods
    pub fn open_reference_picker(&mut self) {
        self.showing_reference_picker = true;
        self.reference_picker_step = ReferencePickerStep::SelectInstance;
        self.reference_selected_instance_index = 0;
        self.reference_selected_output_index = 0;
        self.reference_picker_scroll_offset = 0;
    }

    pub fn close_reference_picker(&mut self) {
        self.showing_reference_picker = false;
        self.reference_picker_step = ReferencePickerStep::SelectInstance;
    }

    /// Get available instances for referencing (excluding current instance)
    fn get_available_reference_instances(&self) -> Vec<(usize, &ModuleInstance)> {
        self.module_instances
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.current_instance_index)
            .collect()
    }

    pub fn next_reference_instance(&mut self) {
        let available = self.get_available_reference_instances();
        if self.reference_selected_instance_index < available.len().saturating_sub(1) {
            self.reference_selected_instance_index += 1;
        }
    }

    pub fn previous_reference_instance(&mut self) {
        if self.reference_selected_instance_index > 0 {
            self.reference_selected_instance_index -= 1;
        }
    }

    pub fn select_reference_instance(&mut self) {
        self.reference_picker_step = ReferencePickerStep::SelectOutput;
        self.reference_selected_output_index = 0;
    }

    pub fn next_reference_output(&mut self) {
        let available = self.get_available_reference_instances();
        if let Some((actual_idx, instance)) = available.get(self.reference_selected_instance_index)
        {
            // Find the module to get its outputs
            if let Some(module) = self
                .available_modules
                .iter()
                .find(|m| m.module_name == instance.module_name && m.version == instance.version)
            {
                if self.reference_selected_output_index < module.tf_outputs.len().saturating_sub(1)
                {
                    self.reference_selected_output_index += 1;
                }
            }
        }
    }

    pub fn previous_reference_output(&mut self) {
        if self.reference_selected_output_index > 0 {
            self.reference_selected_output_index -= 1;
        }
    }

    pub fn back_to_instance_selection(&mut self) {
        self.reference_picker_step = ReferencePickerStep::SelectInstance;
        self.reference_selected_output_index = 0;
    }

    pub fn confirm_reference_selection(&mut self) {
        let available = self.get_available_reference_instances();
        if let Some((actual_idx, instance)) = available.get(self.reference_selected_instance_index)
        {
            if let Some(module) = self
                .available_modules
                .iter()
                .find(|m| m.module_name == instance.module_name && m.version == instance.version)
            {
                if let Some(output) = module.tf_outputs.get(self.reference_selected_output_index) {
                    // Insert the reference at the cursor position
                    let reference = format!(
                        "{{{{ {}::{}::{} }}}}",
                        instance.module_name, instance.instance_name, output.name
                    );

                    if let Some(current_instance) =
                        self.module_instances.get_mut(self.current_instance_index)
                    {
                        if let Some(var) = current_instance
                            .variable_inputs
                            .get_mut(self.selected_variable_index)
                        {
                            var.user_value.insert_str(var.cursor_position, &reference);
                            var.cursor_position += reference.len();
                        }
                    }

                    self.close_reference_picker();
                }
            }
        }
    }
}

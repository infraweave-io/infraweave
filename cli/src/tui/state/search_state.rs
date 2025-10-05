pub struct SearchState {
    pub search_mode: bool,
    pub search_query: String,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            search_mode: false,
            search_query: String::new(),
        }
    }

    pub fn enter_search_mode(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
    }

    pub fn exit_search_mode(&mut self) {
        self.search_mode = false;
        self.search_query.clear();
    }

    pub fn input(&mut self, c: char) {
        self.search_query.push(c);
    }

    pub fn backspace(&mut self) {
        self.search_query.pop();
    }

    pub fn is_active(&self) -> bool {
        self.search_mode && !self.search_query.is_empty()
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

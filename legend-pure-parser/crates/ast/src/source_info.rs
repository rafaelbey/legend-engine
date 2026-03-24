/// Represents source code location for AST nodes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub source_id: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceInfo {
    pub fn new(
        source_id: impl Into<String>,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    pub fn dummy() -> Self {
        Self {
            source_id: String::new(),
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        }
    }
}

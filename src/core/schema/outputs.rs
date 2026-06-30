use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationOutput {
    pub table_roles: HashMap<String, String>,
    pub semantic_types: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptionOutput {
    pub table_descriptions: HashMap<String, String>,
    pub column_descriptions: HashMap<String, String>,
}

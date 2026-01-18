use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfoDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub agent_type: String,
    pub context_window: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelGroupDto {
    pub agent_type: String,
    pub section_title: String,
    pub icon: String,
    pub models: Vec<ModelInfoDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListModelsDto {
    pub groups: Vec<ModelGroupDto>,
}

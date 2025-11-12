use serde::{Deserialize, Serialize};

pub const COLOR_GRAY: &str = "gray";
pub const COLOR_RED: &str = "red";
pub const COLOR_BLUE: &str = "blue";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: u64,
    pub block_hash: String,
    pub timestamp: i64,
    pub parent_ids: Vec<u64>,
    pub daa_score: u64,
    pub height: u64,
    pub height_group_index: u32,
    pub selected_parent_id: Option<u64>,
    pub color: String,
    pub is_in_virtual_selected_parent_chain: bool,
    pub merge_set_red_ids: Vec<u64>,
    pub merge_set_blue_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from_block_id: u64,
    pub to_block_id: u64,
    pub from_height: u64,
    pub to_height: u64,
    pub from_height_group_index: u32,
    pub to_height_group_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeightGroup {
    pub height: u64,
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub id: bool,
    pub tondid_version: String,
    pub processing_version: String,
    pub network: String,
}


use tondi_rpc_core::model::*;
use serde::{Deserialize, Serialize};

// Re-export Tondi RPC types
pub use tondi_rpc_core::model::{
    RpcBlock, RpcHeader, RpcTransaction, RpcHash, 
    GetBlockDAGInfoResponse, GetBlockResponse, GetBlocksResponse,
    GetSinkResponse, GetVirtualChainFromBlockResponse, GetInfoResponse,
};

// Additional wrapper types for compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockAddedNotification {
    pub block: RpcBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualChainChangedNotification {
    pub added_chain_block_hashes: Vec<RpcHash>,
    pub removed_chain_block_hashes: Vec<RpcHash>,
}

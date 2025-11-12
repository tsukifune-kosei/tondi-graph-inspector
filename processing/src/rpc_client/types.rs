// Re-export Tondi RPC types
pub use tondi_rpc_core::model::{
    RpcHash, RpcBlock,
    GetBlockDagInfoResponse, GetBlockResponse, GetBlocksResponse,
    GetSinkResponse, GetVirtualChainFromBlockResponse, GetInfoResponse,
    BlockAddedNotification, VirtualChainChangedNotification,
};

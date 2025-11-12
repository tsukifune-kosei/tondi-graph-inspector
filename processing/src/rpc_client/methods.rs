use crate::rpc_client::{RpcClient, BlockAddedNotification, VirtualChainChangedNotification};
use tondi_rpc_core::api::rpc::RpcApi;
use tondi_rpc_core::model::*;
use tondi_rpc_core::Notification;
use tondi_hashes::Hash;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

impl RpcClient {
    pub async fn get_info(&self) -> Result<GetInfoResponse> {
        let response = self.client.get_info().await
            .map_err(|e| anyhow::anyhow!("GetInfo RPC call failed: {}", e))?;
        Ok(response)
    }

    pub async fn get_block_dag_info(&self) -> Result<GetBlockDagInfoResponse> {
        let response = self.client.get_block_dag_info().await
            .map_err(|e| anyhow::anyhow!("GetBlockDAGInfo RPC call failed: {}", e))?;
        Ok(response)
    }

    pub async fn get_block(&self, hash: &str, include_transactions: bool) -> Result<GetBlockResponse> {
        let rpc_hash: RpcHash = hash.parse::<Hash>()
            .map_err(|e| anyhow::anyhow!("Invalid hash format {}: {}", hash, e))?;
        let block = self.client.get_block(rpc_hash, include_transactions).await
            .map_err(|e| anyhow::anyhow!("GetBlock RPC call failed: {}", e))?;
        Ok(GetBlockResponse { block })
    }

    pub async fn get_blocks(
        &self,
        low_hash: &str,
        include_blocks: bool,
        include_transactions: bool,
    ) -> Result<GetBlocksResponse> {
        let rpc_hash: Option<RpcHash> = if low_hash.is_empty() {
            None
        } else {
            Some(low_hash.parse::<Hash>()
                .map_err(|e| anyhow::anyhow!("Invalid hash format {}: {}", low_hash, e))?)
        };
        let response = self.client.get_blocks(rpc_hash, include_blocks, include_transactions).await
            .map_err(|e| anyhow::anyhow!("GetBlocks RPC call failed: {}", e))?;
        Ok(response)
    }

    pub async fn get_sink(&self) -> Result<GetSinkResponse> {
        let response = self.client.get_sink().await
            .map_err(|e| anyhow::anyhow!("GetSink RPC call failed: {}", e))?;
        Ok(response)
    }

    pub async fn get_virtual_chain_from_block(
        &self,
        start_hash: &str,
        include_accepted_transaction_ids: bool,
    ) -> Result<GetVirtualChainFromBlockResponse> {
        let rpc_hash: RpcHash = start_hash.parse::<Hash>()
            .map_err(|e| anyhow::anyhow!("Invalid hash format {}: {}", start_hash, e))?;
        let response = self.client.get_virtual_chain_from_block(rpc_hash, include_accepted_transaction_ids).await
            .map_err(|e| anyhow::anyhow!("GetVirtualChainFromBlock RPC call failed: {}", e))?;
        Ok(response)
    }

    pub async fn register_for_block_added_notifications<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(BlockAddedNotification) + Send + Sync + 'static,
    {
        // Get the notification channel receiver from the client (Direct mode)
        let receiver = self.client.notification_channel_receiver();
        
        // Start notification for BlockAdded events using Direct mode listener ID
        let listener_id = tondi_grpc_client::GrpcClient::DIRECT_MODE_LISTENER_ID;
        let scope = tondi_notify::scope::Scope::BlockAdded(tondi_notify::scope::BlockAddedScope {});
        
        self.client.start_notify(listener_id, scope).await
            .map_err(|e| anyhow::anyhow!("Failed to start block added notifications: {}", e))?;

        // Spawn a task to handle notifications
        let handler = Arc::new(Mutex::new(handler));
        tokio::spawn(async move {
            while let Ok(notification) = receiver.recv().await {
                match notification {
                    Notification::BlockAdded(notif) => {
                        let h = handler.lock().await;
                        let wrapper = BlockAddedNotification { block: notif.block.clone() };
                        h(wrapper);
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    pub async fn register_for_virtual_chain_changed_notifications<F>(
        &self,
        include_accepted_transaction_ids: bool,
        handler: F,
    ) -> Result<()>
    where
        F: Fn(VirtualChainChangedNotification) + Send + Sync + 'static,
    {
        // Get the notification channel receiver from the client (Direct mode)
        let receiver = self.client.notification_channel_receiver();
        
        // Start notification for VirtualChainChanged events using Direct mode listener ID
        let listener_id = tondi_grpc_client::GrpcClient::DIRECT_MODE_LISTENER_ID;
        let scope = tondi_notify::scope::Scope::VirtualChainChanged(
            tondi_notify::scope::VirtualChainChangedScope::new(include_accepted_transaction_ids)
        );
        
        self.client.start_notify(listener_id, scope).await
            .map_err(|e| anyhow::anyhow!("Failed to start virtual chain changed notifications: {}", e))?;

        // Spawn a task to handle notifications
        let handler = Arc::new(Mutex::new(handler));
        tokio::spawn(async move {
            while let Ok(notification) = receiver.recv().await {
                match notification {
                    Notification::VirtualChainChanged(notif) => {
                        let h = handler.lock().await;
                        let wrapper = VirtualChainChangedNotification {
                            removed_chain_block_hashes: notif.removed_chain_block_hashes.clone(),
                            added_chain_block_hashes: notif.added_chain_block_hashes.clone(),
                            accepted_transaction_ids: notif.accepted_transaction_ids.clone(),
                        };
                        h(wrapper);
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }
}

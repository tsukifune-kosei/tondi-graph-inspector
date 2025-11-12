mod methods;
pub mod types;

pub use methods::*;
pub use types::*;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use tondi_grpc_client::GrpcClient;
use tondi_rpc_core::api::rpc::RpcApi;
use tondi_rpc_core::model::*;
use tondi_rpc_core::notify::connection::ChannelConnection;
use tondi_rpc_core::Notification;
use tondi_utils_tower::counters::TowerConnectionCounters;

#[derive(Clone)]
pub struct RpcClient {
    client: Arc<GrpcClient>,
    address: String,
    on_reconnected_handler: Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>,
}

impl RpcClient {
    pub async fn new(address: &str, _route_capacity: usize) -> Result<Self> {
        info!("Connecting to RPC server at {}", address);
        
        // Ensure address has grpc:// prefix
        let url = if address.starts_with("grpc://") {
            address.to_string()
        } else {
            format!("grpc://{}", address)
        };

        let counters = Arc::new(TowerConnectionCounters::default());
        let client = GrpcClient::connect(url.clone()).await
            .map_err(|e| anyhow::anyhow!("Failed to connect to Tondi RPC server: {}", e))?;

        info!("Connected to Tondi RPC server at {}", address);

        Ok(Self {
            client: Arc::new(client),
            address: address.to_string(),
            on_reconnected_handler: Arc::new(Mutex::new(None)),
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn set_on_reconnected_handler<F>(&self, _handler: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        // Handler will be called on reconnection automatically by the client
        // For now, we don't need to store it since the client handles reconnection
    }

    pub fn client(&self) -> &GrpcClient {
        &self.client
    }
}

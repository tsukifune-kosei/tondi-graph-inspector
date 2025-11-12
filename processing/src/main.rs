mod config;
mod database;
mod processing;
mod rpc_client;
mod version;

use anyhow::Result;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("=================================================");
    println!("Tondi Graph Inspector (TGI)   -   Processing Tier");
    println!("=================================================");

    let config = config::Config::load()?;

    info!("Application version {}", version::VERSION);
    info!("Network {}", config.network());

    let database = database::Database::connect(&config.connection_string).await?;

    let rpc_client = rpc_client::RpcClient::new(config.rpcserver(), 1000).await?;

    let _processing = processing::Processing::new(config, database, rpc_client).await?;

    // Keep the process running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}


use clap::Parser;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
pub struct Config {
    /// Display version information and exit
    #[arg(long)]
    pub show_version: bool,

    /// Directory to store data
    #[arg(short = 'b', long)]
    pub app_dir: Option<String>,

    /// Directory to log output
    #[arg(long)]
    pub log_dir: Option<String>,

    /// Connection string for PostgreSQL database
    /// Format: postgres://<username>:<password>@<host>:<port>/<database>
    #[arg(long)]
    pub connection_string: String,

    /// Connect only to the specified peers at startup
    #[arg(long)]
    pub connect: Vec<String>,

    /// Override DNS seeds with specified hostname
    #[arg(long)]
    pub dnsseed: Option<String>,

    /// Hostname of gRPC server for seeding peers
    #[arg(long)]
    pub grpcseed: Option<String>,

    /// Force to resync all available node blocks with the PostgreSQL database
    #[arg(long)]
    pub resync: bool,

    /// Clear the PostgreSQL database and sync from scratch
    #[arg(long)]
    pub clear_db: bool,

    /// Logging level (trace, debug, info, warn, error)
    #[arg(short = 'd', long, default_value = "info")]
    pub loglevel: String,

    /// RPC server to connect to
    #[arg(short = 's', long)]
    pub rpcserver: Option<String>,

    /// Config file path
    #[arg(short = 'c', long)]
    pub config: Option<String>,

    /// Testnet network suffix number
    #[arg(long)]
    pub netsuffix: Option<u32>,

    /// Network type (mainnet, testnet)
    #[arg(long)]
    pub testnet: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigFile {
    pub connection_string: Option<String>,
    pub rpcserver: Option<String>,
    pub testnet: Option<bool>,
    pub netsuffix: Option<u32>,
    pub loglevel: Option<String>,
    pub resync: Option<bool>,
    pub clear_db: Option<bool>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut config = Config::parse();

        if config.show_version {
            println!("tondi-graph-inspector-processing version {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        // Load config file if specified
        if let Some(config_path) = &config.config {
            let config_file = Self::load_config_file(config_path)?;
            // Override with config file values if not set via CLI
            if config.connection_string.is_empty() {
                config.connection_string = config_file.connection_string.unwrap_or_default();
            }
            if config.rpcserver.is_none() {
                config.rpcserver = config_file.rpcserver;
            }
            if !config.testnet {
                config.testnet = config_file.testnet.unwrap_or(false);
            }
            if config.netsuffix.is_none() {
                config.netsuffix = config_file.netsuffix;
            }
            if config.loglevel == "info" && config_file.loglevel.is_some() {
                config.loglevel = config_file.loglevel.unwrap();
            }
            if !config.resync {
                config.resync = config_file.resync.unwrap_or(false);
            }
            if !config.clear_db {
                config.clear_db = config_file.clear_db.unwrap_or(false);
            }
        }

        // Set default RPC server for testnet if not specified
        if config.rpcserver.is_none() {
            if config.testnet {
                config.rpcserver = Some("grpc://localhost:17110".to_string());
            } else {
                config.rpcserver = Some("grpc://localhost:50051".to_string());
            }
        }

        if config.connection_string.is_empty() {
            anyhow::bail!("--connection-string is required (or set in config file)");
        }

        Ok(config)
    }

    fn load_config_file(path: &str) -> anyhow::Result<ConfigFile> {
        let path = Path::new(path);
        if !path.exists() {
            anyhow::bail!("Config file not found: {}", path.display());
        }
        let contents = fs::read_to_string(path)?;
        let config: ConfigFile = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file: {}", e))?;
        Ok(config)
    }

    pub fn rpcserver(&self) -> &str {
        self.rpcserver.as_deref().unwrap_or("grpc://localhost:50051")
    }

    pub fn network(&self) -> String {
        if self.testnet {
            format!("tondi-testnet{}", self.netsuffix.map(|n| n.to_string()).unwrap_or_default())
        } else {
            "tondi-mainnet".to_string()
        }
    }

    pub fn clear_db(&self) -> bool {
        self.clear_db
    }

    pub fn resync(&self) -> bool {
        self.resync
    }
}


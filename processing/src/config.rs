use clap::Parser;
use serde::Deserialize;
use std::env;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Display version information and exit
    #[arg(short = 'V', long)]
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
    #[arg(short = 's', long, default_value = "localhost")]
    pub rpcserver: String,

    /// Testnet network suffix number
    #[arg(long)]
    pub netsuffix: Option<u32>,

    /// Network type (mainnet, testnet)
    #[arg(long)]
    pub testnet: bool,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut config = Config::parse();

        if config.show_version {
            println!("tondi-graph-inspector-processing version {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        if config.connection_string.is_empty() {
            anyhow::bail!("--connection-string is required");
        }

        Ok(config)
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


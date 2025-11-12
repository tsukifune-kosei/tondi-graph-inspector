use crate::database::Database;
use crate::rpc_client::RpcClient;
use tondi_rpc_core::model::RpcBlock;
use tondi_hashes::Hash;
use anyhow::Result;
use std::collections::HashMap;
use tokio_postgres::Transaction;
use tracing::warn;

const MAX_SUPPORTED_MISSING_DEPENDENCIES: usize = 600;

pub struct Batch {
    database: Database,
    rpc_client: RpcClient,
    blocks: Vec<(String, RpcBlock)>,
    hashes: HashMap<String, usize>, // hash -> index in blocks
    pruning_block: Option<RpcBlock>,
}

impl Batch {
    pub fn new(database: Database, rpc_client: RpcClient, pruning_block: Option<RpcBlock>) -> Self {
        Self {
            database,
            rpc_client,
            blocks: Vec::new(),
            hashes: HashMap::new(),
            pruning_block,
        }
    }

    pub fn in_scope(&self, block: &RpcBlock) -> bool {
        if let Some(ref pruning) = self.pruning_block {
            pruning.header.daa_score <= block.header.daa_score
        } else {
            true
        }
    }

    pub fn has(&self, hash: &str) -> bool {
        self.hashes.contains_key(hash)
    }

    pub fn add(&mut self, hash: String, block: RpcBlock) {
        if !self.has(&hash) && self.in_scope(&block) {
            self.hashes.insert(hash.clone(), self.blocks.len());
            self.blocks.push((hash, block));
        }
    }

    pub fn empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn pop(&mut self) -> Option<(String, RpcBlock)> {
        self.blocks.pop().map(|(hash, block)| {
            self.hashes.remove(&hash);
            (hash, block)
        })
    }

    pub async fn collect_block_and_dependencies(
        &mut self,
        tx: &Transaction<'_>,
        hash: &str,
        block: &RpcBlock,
    ) -> Result<()> {
        self.add(hash.to_string(), block.clone());
        
        let mut i = 0;
        while i < self.blocks.len() {
            let (item_hash, item_block) = &self.blocks[i];
            self.collect_direct_dependencies(tx, item_hash, item_block).await?;
            
            if self.blocks.len() > MAX_SUPPORTED_MISSING_DEPENDENCIES {
                anyhow::bail!(
                    "More than {} missing dependencies found! TGI is out of sync with the node. Terminating the process so it can restart from scratch.",
                    MAX_SUPPORTED_MISSING_DEPENDENCIES
                );
            }
            i += 1;
        }
        Ok(())
    }

    async fn collect_direct_dependencies(
        &mut self,
        tx: &Transaction<'_>,
        hash: &str,
        block: &RpcBlock,
    ) -> Result<()> {
        let parent_hashes = block.header.direct_parents();
        for parent_hash in parent_hashes {
            let parent_hash_str = parent_hash.to_string();
            let parent_exists = self.database.does_block_exist(tx, &parent_hash_str).await?;
            if !parent_exists {
                match self.rpc_client.get_block(&parent_hash_str, false).await {
                    Ok(rpc_block) => {
                        self.add(parent_hash_str.clone(), rpc_block.block);
                        warn!("Missing parent {} of {} registered for processing", parent_hash_str, hash);
                    }
                    Err(e) => {
                        // We ignore the `block not found` error.
                        // In this case the parent is out the node scope so we have no way
                        // to include it in the batch
                        warn!("Parent {} for block {} not found by Tondi domain consensus; the missing dependency is ignored: {}", parent_hash_str, hash, e);
                    }
                }
            }
        }
        Ok(())
    }
}


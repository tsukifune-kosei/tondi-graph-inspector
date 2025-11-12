use crate::database::model::*;
use anyhow::{Context, Result};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls, Transaction};

const BLOCK_BASE_CACHE_CAPACITY: usize = 400000;

#[derive(Clone)]
struct BlockBase {
    id: u64,
    height: u64,
}

pub struct Database {
    client: Arc<Mutex<Client>>,
    block_base_cache: Arc<Mutex<LruCache<String, BlockBase>>>,
}

impl Database {
    pub async fn connect(connection_string: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(connection_string, NoTls).await?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Database connection error: {}", e);
            }
        });

        let cache = LruCache::new(
            NonZeroUsize::new(BLOCK_BASE_CACHE_CAPACITY).unwrap()
        );

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            block_base_cache: Arc::new(Mutex::new(cache)),
        })
    }

    pub async fn run_in_transaction<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Transaction<'_>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<R>> + Send + '_>>,
    {
        let client = self.client.lock().await;
        let transaction = client.transaction().await?;
        let result = f(&transaction).await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn close(&self) -> Result<()> {
        // Connection will close automatically when dropped
        Ok(())
    }

    pub async fn does_block_exist(&self, tx: &Transaction<'_>, block_hash: &str) -> Result<bool> {
        // Check cache first
        {
            let cache = self.block_base_cache.lock().await;
            if cache.peek(block_hash).is_some() {
                return Ok(true);
            }
        }

        // Check database
        let row = tx.query_opt(
            "SELECT id, height FROM blocks WHERE block_hash = $1",
            &[&block_hash],
        ).await?;

        if let Some(row) = row {
            let id: i64 = row.get(0);
            let height: i64 = row.get(1);
            let block_base = BlockBase {
                id: id as u64,
                height: height as u64,
            };
            let mut cache = self.block_base_cache.lock().await;
            cache.put(block_hash.to_string(), block_base);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn insert_block(&self, tx: &Transaction<'_>, block_hash: &str, block: &Block) -> Result<()> {
        let parent_ids_json = serde_json::to_value(&block.parent_ids)?;
        let merge_set_red_ids_json = serde_json::to_value(&block.merge_set_red_ids)?;
        let merge_set_blue_ids_json = serde_json::to_value(&block.merge_set_blue_ids)?;

        let row = tx.query_one(
            r#"
            INSERT INTO blocks (
                block_hash, timestamp, parent_ids, daa_score, height, 
                height_group_index, selected_parent_id, color, 
                is_in_virtual_selected_parent_chain, merge_set_red_ids, merge_set_blue_ids
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id
            "#,
            &[
                &block.block_hash,
                &block.timestamp,
                &parent_ids_json,
                &(block.daa_score as i64),
                &(block.height as i64),
                &(block.height_group_index as i32),
                &block.selected_parent_id.map(|id| id as i64),
                &block.color,
                &block.is_in_virtual_selected_parent_chain,
                &merge_set_red_ids_json,
                &merge_set_blue_ids_json,
            ],
        ).await?;

        let id: i64 = row.get(0);
        let block_base = BlockBase {
            id: id as u64,
            height: block.height,
        };
        let mut cache = self.block_base_cache.lock().await;
        cache.put(block_hash.to_string(), block_base);

        Ok(())
    }

    pub async fn get_block(&self, tx: &Transaction<'_>, id: u64) -> Result<Block> {
        let row = tx.query_one(
            "SELECT * FROM blocks WHERE id = $1",
            &[&(id as i64)],
        ).await?;

        let parent_ids: serde_json::Value = row.get(2);
        let merge_set_red_ids: serde_json::Value = row.get(10);
        let merge_set_blue_ids: serde_json::Value = row.get(11);

        Ok(Block {
            id,
            block_hash: row.get(1),
            timestamp: row.get(2),
            parent_ids: serde_json::from_value(parent_ids)?,
            daa_score: row.get::<_, i64>(4) as u64,
            height: row.get::<_, i64>(5) as u64,
            height_group_index: row.get::<_, i32>(6) as u32,
            selected_parent_id: row.get::<_, Option<i64>>(7).map(|v| v as u64),
            color: row.get(8),
            is_in_virtual_selected_parent_chain: row.get(9),
            merge_set_red_ids: serde_json::from_value(merge_set_red_ids)?,
            merge_set_blue_ids: serde_json::from_value(merge_set_blue_ids)?,
        })
    }

    pub async fn block_id_by_hash(&self, tx: &Transaction<'_>, block_hash: &str) -> Result<u64> {
        // Check cache first
        {
            let cache = self.block_base_cache.lock().await;
            if let Some(block_base) = cache.peek(block_hash) {
                return Ok(block_base.id);
            }
        }

        // Query database
        let row = tx.query_one(
            "SELECT id, height FROM blocks WHERE block_hash = $1",
            &[&block_hash],
        ).await
        .with_context(|| format!("Block hash {} not found in blocks table", block_hash))?;

        let id: i64 = row.get(0);
        let height: i64 = row.get(1);
        let block_base = BlockBase {
            id: id as u64,
            height: height as u64,
        };
        let mut cache = self.block_base_cache.lock().await;
        cache.put(block_hash.to_string(), block_base);

        Ok(id as u64)
    }

    pub async fn block_height_by_hash(&self, tx: &Transaction<'_>, block_hash: &str) -> Result<u64> {
        // Check cache first
        {
            let cache = self.block_base_cache.lock().await;
            if let Some(block_base) = cache.peek(block_hash) {
                return Ok(block_base.height);
            }
        }

        // Query database
        let row = tx.query_one(
            "SELECT id, height FROM blocks WHERE block_hash = $1",
            &[&block_hash],
        ).await
        .with_context(|| format!("Block hash {} not found in blocks table", block_hash))?;

        let id: i64 = row.get(0);
        let height: i64 = row.get(1);
        let block_base = BlockBase {
            id: id as u64,
            height: height as u64,
        };
        let mut cache = self.block_base_cache.lock().await;
        cache.put(block_hash.to_string(), block_base);

        Ok(height as u64)
    }

    pub async fn block_ids_by_hashes(&self, tx: &Transaction<'_>, block_hashes: &[String]) -> Result<Vec<u64>> {
        let mut ids = Vec::with_capacity(block_hashes.len());
        for hash in block_hashes {
            ids.push(self.block_id_by_hash(tx, hash).await?);
        }
        Ok(ids)
    }

    pub async fn block_ids_and_heights_by_hashes(
        &self,
        tx: &Transaction<'_>,
        block_hashes: &[String],
    ) -> Result<(Vec<u64>, Vec<u64>)> {
        let mut ids = Vec::with_capacity(block_hashes.len());
        let mut heights = Vec::with_capacity(block_hashes.len());
        for hash in block_hashes {
            let id = self.block_id_by_hash(tx, hash).await?;
            let height = self.block_height_by_hash(tx, hash).await?;
            ids.push(id);
            heights.push(height);
        }
        Ok((ids, heights))
    }

    pub async fn update_block_selected_parent(&self, tx: &Transaction<'_>, block_id: u64, selected_parent_id: u64) -> Result<()> {
        tx.execute(
            "UPDATE blocks SET selected_parent_id = $1 WHERE id = $2",
            &[&(selected_parent_id as i64), &(block_id as i64)],
        ).await?;
        Ok(())
    }

    pub async fn update_block_merge_set(
        &self,
        tx: &Transaction<'_>,
        block_id: u64,
        merge_set_red_ids: &[u64],
        merge_set_blue_ids: &[u64],
    ) -> Result<()> {
        let red_ids_json = serde_json::to_value(merge_set_red_ids)?;
        let blue_ids_json = serde_json::to_value(merge_set_blue_ids)?;
        tx.execute(
            "UPDATE blocks SET merge_set_red_ids = $1, merge_set_blue_ids = $2 WHERE id = $3",
            &[&red_ids_json, &blue_ids_json, &(block_id as i64)],
        ).await?;
        Ok(())
    }

    pub async fn update_block_is_in_virtual_selected_parent_chain(
        &self,
        tx: &Transaction<'_>,
        block_ids_to_is_in_vspc: &[(u64, bool)],
    ) -> Result<()> {
        for (block_id, is_in_vspc) in block_ids_to_is_in_vspc {
            tx.execute(
                "UPDATE blocks SET is_in_virtual_selected_parent_chain = $1 WHERE id = $2",
                &[is_in_vspc, &(*block_id as i64)],
            ).await?;
        }
        Ok(())
    }

    pub async fn update_block_colors(&self, tx: &Transaction<'_>, block_ids_to_colors: &[(u64, String)]) -> Result<()> {
        for (block_id, color) in block_ids_to_colors {
            tx.execute(
                "UPDATE blocks SET color = $1 WHERE id = $2",
                &[color, &(*block_id as i64)],
            ).await?;
        }
        Ok(())
    }

    pub async fn update_block_daa_scores(&self, tx: &Transaction<'_>, block_ids_to_daa_scores: &[(u64, u64)]) -> Result<()> {
        for (block_id, daa_score) in block_ids_to_daa_scores {
            tx.execute(
                "UPDATE blocks SET daa_score = $1 WHERE id = $2",
                &[&(*daa_score as i64), &(*block_id as i64)],
            ).await?;
        }
        Ok(())
    }

    pub async fn find_latest_stored_block_index(&self, tx: &Transaction<'_>, block_hashes: &[String]) -> Result<usize> {
        // Binary search since hash array is ordered from oldest to latest
        let mut low = 0;
        let mut high = block_hashes.len();
        while (high - low) > 1 {
            let cur = (high + low) / 2;
            let has_block = self.does_block_exist(tx, &block_hashes[cur]).await?;
            if has_block {
                low = cur;
            } else {
                high = cur;
            }
        }
        Ok(low)
    }

    pub async fn block_id_by_daa_score(&self, tx: &Transaction<'_>, daa_score: u64) -> Result<u64> {
        let row = tx.query_one(
            "SELECT id FROM blocks ORDER BY ABS(daa_score - $1) LIMIT 1",
            &[&(daa_score as i64)],
        ).await?;
        Ok(row.get::<_, i64>(0) as u64)
    }

    pub async fn block_count_at_daa_score(&self, tx: &Transaction<'_>, daa_score: u64) -> Result<u32> {
        let row = tx.query_one(
            "SELECT COUNT(*) FROM blocks WHERE daa_score = $1",
            &[&(daa_score as i64)],
        ).await?;
        Ok(row.get::<_, i64>(0) as u32)
    }

    pub async fn highest_block_height(&self, tx: &Transaction<'_>, block_ids: &[u64]) -> Result<u64> {
        let ids: Vec<i64> = block_ids.iter().map(|&id| id as i64).collect();
        let row = tx.query_one(
            "SELECT MAX(height) FROM blocks WHERE id = ANY($1)",
            &[&ids],
        ).await?;
        Ok(row.get::<_, Option<i64>>(0).unwrap_or(0) as u64)
    }

    pub async fn highest_block_in_virtual_selected_parent_chain(&self, tx: &Transaction<'_>) -> Result<Block> {
        let row = tx.query_one(
            "SELECT * FROM blocks WHERE is_in_virtual_selected_parent_chain = $1 ORDER BY height DESC LIMIT 1",
            &[&true],
        ).await?;

        let id: i64 = row.get(0);
        let parent_ids: serde_json::Value = row.get(2);
        let merge_set_red_ids: serde_json::Value = row.get(10);
        let merge_set_blue_ids: serde_json::Value = row.get(11);

        Ok(Block {
            id: id as u64,
            block_hash: row.get(1),
            timestamp: row.get(2),
            parent_ids: serde_json::from_value(parent_ids)?,
            daa_score: row.get::<_, i64>(4) as u64,
            height: row.get::<_, i64>(5) as u64,
            height_group_index: row.get::<_, i32>(6) as u32,
            selected_parent_id: row.get::<_, Option<i64>>(7).map(|v| v as u64),
            color: row.get(8),
            is_in_virtual_selected_parent_chain: row.get(9),
            merge_set_red_ids: serde_json::from_value(merge_set_red_ids)?,
            merge_set_blue_ids: serde_json::from_value(merge_set_blue_ids)?,
        })
    }

    pub async fn height_group_size(&self, tx: &Transaction<'_>, height: u64) -> Result<u32> {
        let row = tx.query_opt(
            "SELECT size FROM height_groups WHERE height = $1",
            &[&(height as i64)],
        ).await?;
        Ok(row.map(|r| r.get::<_, i32>(0) as u32).unwrap_or(0))
    }

    pub async fn block_height(&self, tx: &Transaction<'_>, block_id: u64) -> Result<u64> {
        let row = tx.query_one(
            "SELECT height FROM blocks WHERE id = $1",
            &[&(block_id as i64)],
        ).await?;
        Ok(row.get::<_, i64>(0) as u64)
    }

    pub async fn block_height_group_index(&self, tx: &Transaction<'_>, block_id: u64) -> Result<u32> {
        let row = tx.query_one(
            "SELECT height_group_index FROM blocks WHERE id = $1",
            &[&(block_id as i64)],
        ).await?;
        Ok(row.get::<_, i32>(0) as u32)
    }

    pub async fn insert_edge(&self, tx: &Transaction<'_>, edge: &Edge) -> Result<()> {
        tx.execute(
            r#"
            INSERT INTO edges (from_block_id, to_block_id, from_height, to_height, from_height_group_index, to_height_group_index)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (from_block_id, to_block_id) DO NOTHING
            "#,
            &[
                &(edge.from_block_id as i64),
                &(edge.to_block_id as i64),
                &(edge.from_height as i64),
                &(edge.to_height as i64),
                &(edge.from_height_group_index as i32),
                &(edge.to_height_group_index as i32),
            ],
        ).await?;
        Ok(())
    }

    pub async fn insert_or_update_height_group(&self, tx: &Transaction<'_>, height_group: &HeightGroup) -> Result<()> {
        tx.execute(
            r#"
            INSERT INTO height_groups (height, size)
            VALUES ($1, $2)
            ON CONFLICT (height) DO UPDATE SET size = EXCLUDED.size
            "#,
            &[&(height_group.height as i64), &(height_group.size as i32)],
        ).await?;
        Ok(())
    }

    pub async fn get_app_config(&self, tx: &Transaction<'_>) -> Result<AppConfig> {
        let row = tx.query_one(
            "SELECT * FROM app_config WHERE id = $1",
            &[&true],
        ).await?;

        Ok(AppConfig {
            id: row.get(0),
            tondid_version: row.get(1),
            processing_version: row.get(2),
            network: row.get(3),
        })
    }

    pub async fn store_app_config(&self, tx: &Transaction<'_>, config: &AppConfig) -> Result<()> {
        tx.execute(
            r#"
            INSERT INTO app_config (id, tondid_version, processing_version, network)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET
                tondid_version = EXCLUDED.tondid_version,
                processing_version = EXCLUDED.processing_version,
                network = EXCLUDED.network
            "#,
            &[&true, &config.tondid_version, &config.processing_version, &config.network],
        ).await?;
        Ok(())
    }

    pub async fn clear(&self, tx: &Transaction<'_>) -> Result<()> {
        let mut cache = self.block_base_cache.lock().await;
        cache.clear();
        drop(cache);

        tx.execute("TRUNCATE TABLE blocks", &[]).await?;
        tx.execute("TRUNCATE TABLE edges", &[]).await?;
        tx.execute("TRUNCATE TABLE height_groups", &[]).await?;
        Ok(())
    }

    pub async fn load_cache(&self, tx: &Transaction<'_>, min_height: u64) -> Result<()> {
        let rows = tx.query(
            "SELECT id, block_hash, height FROM blocks WHERE height >= $1",
            &[&(min_height as i64)],
        ).await?;

        let mut cache = self.block_base_cache.lock().await;
        cache.clear();

        for row in rows {
            let id: i64 = row.get(0);
            let block_hash: String = row.get(1);
            let height: i64 = row.get(2);
            cache.put(block_hash, BlockBase {
                id: id as u64,
                height: height as u64,
            });
        }

        Ok(())
    }
}


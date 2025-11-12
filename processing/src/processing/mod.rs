mod batch;

use crate::config::Config;
use crate::database::{Database, Block, Edge, HeightGroup, AppConfig};
use crate::rpc_client::{RpcClient, GetBlockDagInfoResponse};
use crate::rpc_client::types::{BlockAddedNotification, VirtualChainChangedNotification};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use tondi_rpc_core::model::RpcBlock;
use tondi_hashes::Hash;

pub struct Processing {
    config: Config,
    database: Database,
    rpc_client: Arc<RpcClient>,
    app_config: Arc<Mutex<AppConfig>>,
    syncing: Arc<Mutex<bool>>,
}

impl Processing {
    pub async fn new(config: Config, database: Database, rpc_client: RpcClient) -> Result<Self> {
        let app_config = Arc::new(Mutex::new(AppConfig {
            id: true,
            tondid_version: "unknown".to_string(),
            processing_version: env!("CARGO_PKG_VERSION").to_string(),
            network: config.network(),
        }));

        let processing = Self {
            config,
            database,
            rpc_client: Arc::new(rpc_client),
            app_config,
            syncing: Arc::new(Mutex::new(false)),
        };

        processing.init().await?;

        Ok(processing)
    }

    async fn init(&self) -> Result<()> {
        self.update_rpc_client_version().await?;
        self.register_app_config().await?;
        self.wait_for_synced_rpc_client().await?;
        self.resync_database().await?;
        self.initialize_consensus_events_handler().await?;
        Ok(())
    }

    async fn update_rpc_client_version(&self) -> Result<()> {
        let info = self.rpc_client.get_info().await?;
        let mut app_config = self.app_config.lock().await;
        app_config.tondid_version = info.server_version;
        Ok(())
    }

    async fn register_app_config(&self) -> Result<()> {
        info!("Registering app config");
        let app_config = self.app_config.lock().await.clone();
        info!(
            "Config = TGI version: {}, Node version: {}, Network: {}",
            app_config.processing_version, app_config.tondid_version, app_config.network
        );

        let app_config_clone = app_config.clone();
        let database = self.database.clone();
        self.database.run_in_transaction(move |tx| {
            let app_config = app_config_clone.clone();
            let database = database.clone();
            Box::pin(async move {
                database.store_app_config(tx, &app_config).await
            })
        }).await?;

        info!("Finished registering app config");
        Ok(())
    }

    async fn wait_for_synced_rpc_client(&self) -> Result<()> {
        let mut cycle = 0;
        loop {
            let info = self.rpc_client.get_info().await?;
            if info.is_synced {
                info!("Node is synced");
                return Ok(());
            }
            if cycle == 0 {
                info!("Waiting for the node to finish IBD...");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            cycle += 1;
        }
    }

    async fn resync_database(&self) -> Result<()> {
        let mut syncing = self.syncing.lock().await;
        *syncing = true;
        drop(syncing);

        let rpc_client = self.rpc_client.clone();
        let database = self.database.clone();
        let config_clear_db = self.config.clear_db();
        let config_resync = self.config.resync();

        self.database.run_in_transaction(move |tx| {
            Box::pin(async move {
                info!("Resyncing database");
                
                let dag_info = rpc_client.get_block_dag_info().await?;
                let pruning_point_hash_str = dag_info.pruning_point_hash.to_string();
                
                let pruning_block_resp = rpc_client.get_block(&pruning_point_hash_str, false).await?;
                let pruning_block = pruning_block_resp.block;
                
                let has_pruning_block = database.does_block_exist(tx, &pruning_point_hash_str).await?;
                
                let mut low_hash = pruning_point_hash_str.clone();
                let mut keep_database = has_pruning_block && !config_clear_db;
                
                if keep_database {
                    info!("Pruning point {} already in the database", pruning_point_hash_str);
                    info!("Database kept");
                    
                    let pruning_block_height = database.block_height_by_hash(tx, &pruning_point_hash_str).await?;
                    
                    info!("Loading cache");
                    database.load_cache(tx, pruning_block_height).await?;
                    info!("Cache loaded from the database");
                    
                    info!("Searching for an optimal sync starting point");
                    low_hash = Self::find_optimal_sync_starting_block(
                        &database, tx, &rpc_client, &pruning_point_hash_str, 
                        pruning_block.header.daa_score
                    ).await?;
                    if low_hash != pruning_point_hash_str {
                        info!("Optimal sync starting point set at {}", low_hash);
                    } else {
                        info!("Sync starting point set at the pruning point");
                    }
                } else {
                    database.clear(tx).await?;
                    info!("Database cleared");
                    
                    let pruning_database_block = Block {
                        id: 0,
                        block_hash: pruning_point_hash_str.clone(),
                        timestamp: pruning_block.header.timestamp as i64,
                        parent_ids: vec![],
                        daa_score: pruning_block.header.daa_score,
                        height: 0,
                        height_group_index: 0,
                        selected_parent_id: None,
                        color: "gray".to_string(),
                        is_in_virtual_selected_parent_chain: true,
                        merge_set_red_ids: vec![],
                        merge_set_blue_ids: vec![],
                    };
                    database.insert_block(tx, &pruning_point_hash_str, &pruning_database_block).await?;
                    
                    let height_group = HeightGroup {
                        height: 0,
                        size: 1,
                    };
                    database.insert_or_update_height_group(tx, &height_group).await?;
                    info!("Pruning point {} has been added to the database", pruning_point_hash_str);
                }

                let mut vspc_cycle = 0;
                loop {
                    info!("Cycle {} - Load node blocks", vspc_cycle);
                    let hashes = Self::get_hashes_to_selected_tip(
                        &rpc_client, &low_hash, dag_info.virtual_daa_score, 
                        pruning_block.header.daa_score
                    ).await?;
                    info!("Cycle {} - Node blocks loaded", vspc_cycle);

                    let mut start_index = 0;
                    if keep_database && vspc_cycle == 0 {
                        info!("Cycle {} - Syncing {} blocks with the database", vspc_cycle, hashes.len());
                        if !config_resync {
                            start_index = database.find_latest_stored_block_index(tx, &hashes).await?;
                            info!("Cycle {} - First {} blocks already exist in the database", vspc_cycle, start_index);
                            start_index = start_index.saturating_sub(3000usize);
                        }
                    } else {
                        info!("Cycle {} - Adding {} blocks to the database", vspc_cycle, hashes.len());
                    }

                    let total_to_add = hashes.len() - start_index;
                    for i in start_index..hashes.len() {
                        let block_hash = &hashes[i];
                        let rpc_block_resp = rpc_client.get_block(block_hash, false).await?;
                        let rpc_block = rpc_block_resp.block;
                        
                        if config_resync || (i - start_index) >= 6000 {
                            Self::process_block_static(&database, tx, &rpc_client, &rpc_block, None).await?;
                        } else {
                            Self::process_block_and_dependencies_static(
                                &database, tx, &rpc_client, block_hash, &rpc_block, Some(&pruning_block)
                            ).await?;
                        }
                        
                        let added_count = i + 1 - start_index;
                        if added_count % 1000 == 0 || added_count == total_to_add {
                            info!("Cycle {} - Added {}/{} blocks to the database", vspc_cycle, added_count, total_to_add);
                        }
                    }

                    if hashes.len() < 20 {
                        Self::resync_virtual_selected_parent_chain_static(&database, tx, &rpc_client, true).await?;
                        vspc_cycle += 1;
                    }

                    if vspc_cycle > 1 && hashes.len() < 10 {
                        info!("Cycle {} - Almost at tip with last {} blocks added, stopping resync", vspc_cycle, hashes.len());
                        break;
                    }

                    keep_database = true;
                }

                info!("Finished resyncing database");
                Ok(())
            })
        }).await?;

        let mut syncing = self.syncing.lock().await;
        *syncing = false;
        Ok(())
    }

    async fn find_optimal_sync_starting_block(
        database: &Database,
        tx: &tokio_postgres::Transaction<'_>,
        rpc_client: &RpcClient,
        pruning_point_hash: &str,
        pruning_point_daa_score: u64,
    ) -> Result<String> {
        // Simplified version - in full implementation would search backwards
        Ok(pruning_point_hash.to_string())
    }

    async fn get_hashes_to_selected_tip(
        rpc_client: &RpcClient,
        low_hash: &str,
        virtual_daa_score: u64,
        _pruning_point_daa_score: u64,
    ) -> Result<Vec<String>> {
        let response = rpc_client.get_blocks(low_hash, false, false).await?;
        Ok(response.block_hashes.iter().map(|h| h.to_string()).collect())
    }

    async fn process_block_and_dependencies_static(
        database: &Database,
        tx: &tokio_postgres::Transaction<'_>,
        rpc_client: &RpcClient,
        hash: &str,
        block: &RpcBlock,
        pruning_block: Option<&RpcBlock>,
    ) -> Result<()> {
        let mut batch = batch::Batch::new(
            database.clone(),
            rpc_client.clone(),
            pruning_block.cloned(),
        );
        batch.collect_block_and_dependencies(tx, hash, block).await?;
        
        while let Some((_hash, block)) = batch.pop() {
            if !batch.empty() {
                warn!("Handling missing dependency block {}", _hash);
            }
            Self::process_block_static(database, tx, rpc_client, &block, None).await?;
        }
        Ok(())
    }

    async fn process_block_static(
        database: &Database,
        tx: &tokio_postgres::Transaction<'_>,
        rpc_client: &RpcClient,
        block: &RpcBlock,
        _pruning_block: Option<&RpcBlock>,
    ) -> Result<()> {
        let block_hash = block.header.hash.to_string();
        debug!("Processing block {}", block_hash);
        
        let block_exists = database.does_block_exist(tx, &block_hash).await?;
        
        if !block_exists {
            let parent_hashes = block.header.direct_parents();
            let mut existing_parent_hashes = Vec::new();
            for parent_hash in parent_hashes {
                let parent_hash_str = parent_hash.to_string();
                let parent_exists = database.does_block_exist(tx, &parent_hash_str).await?;
                if parent_exists {
                    existing_parent_hashes.push(parent_hash_str);
                } else {
                    warn!("Parent {} for block {} does not exist in the database", parent_hash_str, block_hash);
                }
            }

            let (parent_ids, parent_heights) = database.block_ids_and_heights_by_hashes(tx, &existing_parent_hashes).await?;

            let block_height = parent_heights.iter().max().map(|&h| h + 1).unwrap_or(0);
            let height_group_size = database.height_group_size(tx, block_height).await?;
            let block_height_group_index = height_group_size;

            let database_block = Block {
                id: 0,
                block_hash: block_hash.clone(),
                timestamp: block.header.timestamp as i64,
                parent_ids,
                height: block_height,
                height_group_index: block_height_group_index as u32,
                selected_parent_id: None,
                color: "gray".to_string(),
                is_in_virtual_selected_parent_chain: false,
                merge_set_red_ids: vec![],
                merge_set_blue_ids: vec![],
                daa_score: block.header.daa_score,
            };
            database.insert_block(tx, &block_hash, &database_block).await?;

            let block_id = database.block_id_by_hash(tx, &block_hash).await?;
            let height_group = HeightGroup {
                height: block_height,
                size: (block_height_group_index + 1) as u32,
            };
            database.insert_or_update_height_group(tx, &height_group).await?;

            for parent_id in &database_block.parent_ids {
                let parent_height = database.block_height(tx, *parent_id).await?;
                let parent_height_group_index = database.block_height_group_index(tx, *parent_id).await?;
                let edge = Edge {
                    from_block_id: block_id,
                    to_block_id: *parent_id,
                    from_height: block_height,
                    to_height: parent_height,
                    from_height_group_index: block_height_group_index as u32,
                    to_height_group_index: parent_height_group_index as u32,
                };
                database.insert_edge(tx, &edge).await?;
            }
        } else {
            debug!("Block {} already exists in database; not processed", block_hash);
        }

        let rpc_block_resp = rpc_client.get_block(&block_hash, false).await?;
        let verbose_data = match rpc_block_resp.block.verbose_data {
            Some(vd) => vd,
            None => {
                warn!("Block {} is incomplete so leaving block processing", block_hash);
                return Ok(());
            }
        };

        if verbose_data.is_header_only {
            warn!("Block {} is incomplete so leaving block processing", block_hash);
            return Ok(());
        }

        let selected_parent_hash_str = verbose_data.selected_parent_hash.to_string();
        let selected_parent_id = database.block_id_by_hash(tx, &selected_parent_hash_str).await
            .with_context(|| format!("Could not get id of selected parent block {}", selected_parent_hash_str))?;

        let block_id = database.block_id_by_hash(tx, &block_hash).await
            .with_context(|| format!("Could not get id of block {}", block_hash))?;

        database.update_block_selected_parent(tx, block_id, selected_parent_id).await
            .with_context(|| format!("Could not update selected parent of block {}", block_hash))?;

        let merge_set_reds: Vec<String> = verbose_data.merge_set_reds_hashes.iter().map(|h| h.to_string()).collect();
        let merge_set_red_ids = database.block_ids_by_hashes(tx, &merge_set_reds).await.unwrap_or_default();

        let merge_set_blues: Vec<String> = verbose_data.merge_set_blues_hashes.iter().map(|h| h.to_string()).collect();
        let merge_set_blue_ids = database.block_ids_by_hashes(tx, &merge_set_blues).await.unwrap_or_default();

        database.update_block_merge_set(tx, block_id, &merge_set_red_ids, &merge_set_blue_ids).await
            .with_context(|| format!("Could not update merge sets colors for block {}", block_hash))?;

        debug!("Finished processing block {}", block_hash);
        Ok(())
    }

    async fn resync_virtual_selected_parent_chain_static(
        database: &Database,
        tx: &tokio_postgres::Transaction<'_>,
        rpc_client: &RpcClient,
        _with_dependencies: bool,
    ) -> Result<()> {
        let sink_resp = rpc_client.get_sink().await?;
        let sink_hash_str = sink_resp.sink.to_string();
        
        let virtual_chain_resp = rpc_client.get_virtual_chain_from_block(&sink_hash_str, false).await?;
        
        let mut block_is_in_virtual_selected_parent_chain: HashMap<u64, bool> = HashMap::new();
        
        for removed_hash in &virtual_chain_resp.removed_chain_block_hashes {
            let removed_hash_str = removed_hash.to_string();
            if let Ok(removed_block_id) = database.block_id_by_hash(tx, &removed_hash_str).await {
                block_is_in_virtual_selected_parent_chain.insert(removed_block_id, false);
            }
        }
        
        for added_hash in &virtual_chain_resp.added_chain_block_hashes {
            let added_hash_str = added_hash.to_string();
            if let Ok(added_block_id) = database.block_id_by_hash(tx, &added_hash_str).await {
                block_is_in_virtual_selected_parent_chain.insert(added_block_id, true);
            }
        }
        
        let updates: Vec<(u64, bool)> = block_is_in_virtual_selected_parent_chain.iter().map(|(k, v)| (*k, *v)).collect();
        database.update_block_is_in_virtual_selected_parent_chain(tx, &updates).await?;
        
        let mut block_colors: HashMap<u64, String> = HashMap::new();
        
        for added_hash in &virtual_chain_resp.added_chain_block_hashes {
            let added_hash_str = added_hash.to_string();
            let rpc_block_resp = rpc_client.get_block(&added_hash_str, false).await?;
            if let Some(verbose_data) = rpc_block_resp.block.verbose_data {
                for blue_hash in &verbose_data.merge_set_blues_hashes {
                    let blue_hash_str = blue_hash.to_string();
                    if let Ok(blue_block_id) = database.block_id_by_hash(tx, &blue_hash_str).await {
                        block_colors.insert(blue_block_id, "blue".to_string());
                    }
                }
                for red_hash in &verbose_data.merge_set_reds_hashes {
                    let red_hash_str = red_hash.to_string();
                    if let Ok(red_block_id) = database.block_id_by_hash(tx, &red_hash_str).await {
                        block_colors.insert(red_block_id, "red".to_string());
                    }
                }
            }
        }
        
        let color_updates: Vec<(u64, String)> = block_colors.iter().map(|(k, v)| (*k, v.clone())).collect();
        database.update_block_colors(tx, &color_updates).await?;
        
        info!("Updated the virtual selected parent chain");
        Ok(())
    }

    async fn initialize_consensus_events_handler(&self) -> Result<()> {
        let database1 = self.database.clone();
        let rpc_client1 = self.rpc_client.clone();
        
        let database1_clone = database1.clone();
        let rpc_client1_clone = rpc_client1.clone();
        rpc_client1.register_for_block_added_notifications(move |notification: BlockAddedNotification| {
            let database = database1_clone.clone();
            let rpc_client = rpc_client1_clone.clone();
            let block = (*notification.block).clone();
            tokio::spawn(async move {
                if let Err(e) = Self::process_block_notification(&database, &rpc_client, &block).await {
                    warn!("Error processing block added notification: {}", e);
                }
            });
        }).await?;

        let database2 = self.database.clone();
        let rpc_client2 = self.rpc_client.clone();
        
        let database2_clone = database2.clone();
        let rpc_client2_clone = rpc_client2.clone();
        rpc_client2.register_for_virtual_chain_changed_notifications(false, move |notification: VirtualChainChangedNotification| {
            let database = database2_clone.clone();
            let rpc_client = rpc_client2_clone.clone();
            let notification = notification.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::process_virtual_chain_changed_notification(&database, &rpc_client, notification).await {
                    warn!("Error processing virtual chain changed notification: {}", e);
                }
            });
        }).await?;

        Ok(())
    }

    async fn process_block_notification(
        database: &Database,
        rpc_client: &RpcClient,
        block: &RpcBlock,
    ) -> Result<()> {
        let block_hash = block.header.hash.to_string();
        let block = block.clone();
        let database = database.clone();
        let rpc_client = rpc_client.clone();
        let database_for_closure = database.clone();
        let rpc_client_for_closure = rpc_client.clone();
        database.run_in_transaction(move |tx| {
            let block = block.clone();
            let block_hash = block_hash.clone();
            let rpc_client = rpc_client_for_closure.clone();
            let database = database_for_closure.clone();
            Box::pin(async move {
                Self::process_block_and_dependencies_static(&database, tx, &rpc_client, &block_hash, &block, None).await
            })
        }).await
    }

    async fn process_virtual_chain_changed_notification(
        database: &Database,
        rpc_client: &RpcClient,
        notification: VirtualChainChangedNotification,
    ) -> Result<()> {
        let notification = notification.clone();
        let database = database.clone();
        let rpc_client = rpc_client.clone();
        let database_for_closure = database.clone();
        let rpc_client_for_closure = rpc_client.clone();
        database.run_in_transaction(move |tx| {
            let notification = notification.clone();
            let rpc_client = rpc_client_for_closure.clone();
            let database = database_for_closure.clone();
            Box::pin(async move {
                let mut block_is_in_virtual_selected_parent_chain: HashMap<u64, bool> = HashMap::new();
                
                for removed_hash in notification.removed_chain_block_hashes.iter() {
                    let removed_hash_str = removed_hash.to_string();
                    if let Ok(removed_block_id) = database.block_id_by_hash(tx, &removed_hash_str).await {
                        block_is_in_virtual_selected_parent_chain.insert(removed_block_id, false);
                    }
                }
                
                for added_hash in notification.added_chain_block_hashes.iter() {
                    let added_hash_str = added_hash.to_string();
                    if let Ok(added_block_id) = database.block_id_by_hash(tx, &added_hash_str).await {
                        block_is_in_virtual_selected_parent_chain.insert(added_block_id, true);
                    }
                }
                
                let updates: Vec<(u64, bool)> = block_is_in_virtual_selected_parent_chain.iter().map(|(k, v)| (*k, *v)).collect();
        database.update_block_is_in_virtual_selected_parent_chain(tx, &updates).await?;
                
                let mut block_colors: HashMap<u64, String> = HashMap::new();
                
                for added_hash in notification.added_chain_block_hashes.iter() {
                    let added_hash_str = added_hash.to_string();
                    let rpc_block_resp = rpc_client.get_block(&added_hash_str, false).await?;
                    if let Some(verbose_data) = rpc_block_resp.block.verbose_data {
                        for blue_hash in &verbose_data.merge_set_blues_hashes {
                            let blue_hash_str = blue_hash.to_string();
                            if let Ok(blue_block_id) = database.block_id_by_hash(tx, &blue_hash_str).await {
                                block_colors.insert(blue_block_id, "blue".to_string());
                            }
                        }
                        for red_hash in &verbose_data.merge_set_reds_hashes {
                            let red_hash_str = red_hash.to_string();
                            if let Ok(red_block_id) = database.block_id_by_hash(tx, &red_hash_str).await {
                                block_colors.insert(red_block_id, "red".to_string());
                            }
                        }
                    }
                }
                
                let color_updates: Vec<(u64, String)> = block_colors.iter().map(|(k, v)| (*k, v.clone())).collect();
        database.update_block_colors(tx, &color_updates).await?;
                Ok(())
            })
        }).await
    }
}

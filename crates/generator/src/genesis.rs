use crate::traits::StateExt;
use anyhow::Result;
use gw_common::{
    blake2b::new_blake2b,
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    smt::H256,
    state::State,
    CKB_SUDT_SCRIPT_ARGS,
};
use gw_config::GenesisConfig;
use gw_store::{
    state::state_db::{StateContext, StateTree},
    transaction::StoreTransaction,
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::{ScriptHashType, Status},
    offchain::RollupContext,
    packed::{
        AccountMerkleState, BlockMerkleState, GlobalState, L2Block, L2BlockCommittedInfo,
        RawL2Block, Script, SubmitTransactions,
    },
    prelude::*,
};

/// Build genesis block
pub fn build_genesis(config: &GenesisConfig, secp_data: Bytes) -> Result<GenesisWithGlobalState> {
    let store = Store::open_tmp()?;
    let db = store.begin_transaction();
    build_genesis_from_store(db, config, secp_data)
        .map(|(_db, genesis_with_state)| genesis_with_state)
}

pub struct GenesisWithGlobalState {
    pub genesis: L2Block,
    pub global_state: GlobalState,
}

/// build genesis from store
/// This function initialize db to genesis state
pub fn build_genesis_from_store(
    db: StoreTransaction,
    config: &GenesisConfig,
    secp_data: Bytes,
) -> Result<(StoreTransaction, GenesisWithGlobalState)> {
    let rollup_context = RollupContext {
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.rollup_type_hash.clone().into();
            rollup_script_hash.into()
        },
        rollup_config: config.rollup_config.clone().into(),
    };
    // initialize store
    db.set_block_smt_root(H256::zero())?;
    db.set_reverted_block_smt_root(H256::zero())?;

    let mut tree = {
        let smt = db.account_smt_with_merkle_state(AccountMerkleState::default())?;
        StateTree::new(smt, 0, StateContext::AttachBlock(0))
    };

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_id = tree.create_account_from_script(
        Script::new_builder()
            .code_hash({
                let code_hash: [u8; 32] = config.meta_contract_validator_type_hash.clone().into();
                code_hash.pack()
            })
            .hash_type(ScriptHashType::Type.into())
            .args({
                let rollup_script_hash: [u8; 32] = rollup_context.rollup_script_hash.into();
                Bytes::from(rollup_script_hash.to_vec()).pack()
            })
            .build(),
    )?;
    assert_eq!(
        reserved_id, RESERVED_ACCOUNT_ID,
        "reserved account id must be zero"
    );

    // setup CKB simple UDT contract
    let ckb_sudt_script =
        crate::sudt::build_l2_sudt_script(&rollup_context, &CKB_SUDT_SCRIPT_ARGS.into());
    let ckb_sudt_id = tree.create_account_from_script(ckb_sudt_script)?;
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );

    #[cfg(feature = "generate-genesis-accounts")]
    {
        crate::genesis_accounts::load_and_generate_genesis_accounts(&mut tree, &rollup_context);
    }

    let prev_state_checkpoint: [u8; 32] = tree.calculate_state_checkpoint()?.into();
    let submit_txs = SubmitTransactions::new_builder()
        .prev_state_checkpoint(prev_state_checkpoint.pack())
        .build();

    // calculate post state
    let post_account = {
        let root = tree.calculate_root()?;
        let count = tree.get_account_count()?;
        AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build()
    };

    let raw_genesis = RawL2Block::new_builder()
        .number(0u64.pack())
        .block_producer_id(0u32.pack())
        .parent_block_hash([0u8; 32].pack())
        .timestamp(config.timestamp.pack())
        .post_account(post_account.clone())
        .submit_transactions(submit_txs)
        .build();

    // generate block proof
    let genesis_hash = raw_genesis.hash();
    let (block_root, block_proof) = {
        let block_key = RawL2Block::compute_smt_key(0);
        let mut smt = db.block_smt()?;
        smt.update(block_key.into(), genesis_hash.into())?;
        let block_proof = smt
            .merkle_proof(vec![block_key.into()])?
            .compile(vec![(block_key.into(), genesis_hash.into())])?;
        let block_root = *smt.root();
        (block_root, block_proof)
    };

    // build genesis
    let genesis = L2Block::new_builder()
        .raw(raw_genesis)
        .block_proof(block_proof.0.pack())
        .build();
    let global_state = {
        let post_block = BlockMerkleState::new_builder()
            .merkle_root({
                let root: [u8; 32] = block_root.into();
                root.pack()
            })
            .count(1u64.pack())
            .build();
        let rollup_config_hash = {
            let mut hasher = new_blake2b();
            hasher.update(rollup_context.rollup_config.as_slice());
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };
        GlobalState::new_builder()
            .account(post_account)
            .block(post_block)
            .status((Status::Running as u8).into())
            .rollup_config_hash(rollup_config_hash.pack())
            .tip_block_hash(genesis.hash().pack())
            .version(1.into())
            .build()
    };

    // insert secp256k1 data
    let secp_data_hash = {
        let mut hasher = new_blake2b();
        hasher.update(secp_data.as_ref());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    };
    tree.insert_data(secp_data_hash.into(), secp_data);

    db.set_block_smt_root(global_state.block().merkle_root().unpack())?;
    let genesis_with_global_state = GenesisWithGlobalState {
        genesis,
        global_state,
    };
    Ok((db, genesis_with_global_state))
}

pub fn init_genesis(
    store: &Store,
    config: &GenesisConfig,
    genesis_committed_info: L2BlockCommittedInfo,
    secp_data: Bytes,
) -> Result<()> {
    let rollup_script_hash: H256 = {
        let rollup_script_hash: [u8; 32] = config.rollup_type_hash.clone().into();
        rollup_script_hash.into()
    };
    if store.has_genesis()? {
        let chain_id = store.get_chain_id()?;
        if chain_id == rollup_script_hash {
            return Ok(());
        } else {
            panic!(
                "The store is already initialized by rollup_type_hash: 0x{}!",
                hex::encode(chain_id.as_slice())
            );
        }
    }
    let db = store.begin_transaction();
    db.setup_chain_id(rollup_script_hash)?;
    let (
        db,
        GenesisWithGlobalState {
            genesis,
            global_state,
        },
    ) = build_genesis_from_store(db, config, secp_data)?;
    let prev_txs_state = genesis.as_reader().raw().post_account().to_entity();
    db.insert_block(
        genesis.clone(),
        global_state,
        Vec::new(),
        prev_txs_state,
        Vec::new(),
        Vec::new(),
    )?;
    db.insert_block_committed_info(&genesis.hash().into(), genesis_committed_info)?;
    db.attach_block(genesis)?;
    db.commit()?;
    Ok(())
}

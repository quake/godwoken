use gw_common::H256;
use gw_db::{
    error::Error,
    iter::DBIter,
    schema::{
        Col, COLUMN_MEM_POOL_TRANSACTION_RECEIPT, COLUMN_TRANSACTION_INFO,
        COLUMN_TRANSACTION_RECEIPT,
    },
    DBIterator, IteratorMode, ReadOptions, RocksDBSnapshot,
};
use gw_types::{
    packed::{self, TransactionKey},
    prelude::*,
};

use crate::traits::KVStore;

pub struct StoreSnapshot {
    pub(crate) inner: RocksDBSnapshot,
}

impl KVStore for StoreSnapshot {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner
            .get_pinned(col, key)
            .expect("snapshot operation should be ok")
            .map(|v| Box::<[u8]>::from(v.as_ref()))
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }

    fn get_iter_opts(&self, col: Col, mode: IteratorMode, opts: &ReadOptions) -> DBIter {
        self.inner
            .iter_opt(col, mode, opts)
            .expect("db operation should be ok")
    }

    fn insert_raw(&self, _col: Col, _key: &[u8], _value: &[u8]) -> Result<(), Error> {
        unreachable!("snapshot should not be writable");
    }

    fn delete(&self, _col: Col, _key: &[u8]) -> Result<(), Error> {
        unreachable!("snapshot should not be writable");
    }
}

// TODO: refactor this to extract same code with the `StoreTransaction` implementations
impl StoreSnapshot {
    pub fn get_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        if let Some(slice) = self.get(COLUMN_TRANSACTION_INFO, tx_hash.as_slice()) {
            let info =
                packed::TransactionInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity();
            let tx_key = info.key();
            self.get_transaction_receipt_by_key(&tx_key)
        } else {
            Ok(None)
        }
    }

    pub fn get_transaction_receipt_by_key(
        &self,
        key: &TransactionKey,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_TRANSACTION_RECEIPT, key.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    pub fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }
}

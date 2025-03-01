use async_jsonrpc_client::Output;
use gw_common::H256;
use serde::de::DeserializeOwned;
use serde_json::from_value;

pub(crate) const DEFAULT_QUERY_LIMIT: usize = 500;

lazy_static::lazy_static! {
    /// CKB built-in type ID code hash
    pub(crate) static ref TYPE_ID_CODE_HASH: [u8; 32] = {
        let hexed_type_id_code_hash: &str = "00000000000000000000000000000000000000000000000000545950455f4944";
        let mut code_hash = [0u8; 32];
        faster_hex::hex_decode(hexed_type_id_code_hash.as_bytes(), &mut code_hash).expect("dehex type id code_hash");
        code_hash
    };
}

pub(crate) type JsonH256 = ckb_fixed_hash::H256;

pub(crate) fn to_h256(v: JsonH256) -> H256 {
    let h: [u8; 32] = v.into();
    h.into()
}

pub(crate) fn to_jsonh256(v: H256) -> JsonH256 {
    let h: [u8; 32] = v.into();
    h.into()
}

pub(crate) fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow::anyhow!("JSONRPC error: {}", failure.error)),
    }
}

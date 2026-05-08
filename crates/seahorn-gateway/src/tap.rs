//! TAP v2 (GraphTally) — inlined types, EIP-712 hashing, and receipt validation.
//!
//! Inlined from dispatch-tap rather than taken as a dependency, keeping
//! seahorn-gateway fully self-contained.

use alloy_primitives::{keccak256, Address, Bytes, B256, U256};
use alloy_sol_types::SolValue;
use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey};
use serde::{Deserialize, Serialize};

// ── Type strings ──────────────────────────────────────────────────────────────

const RECEIPT_TYPE_STRING: &str =
    "Receipt(address data_service,address service_provider,uint64 timestamp_ns,uint64 nonce,uint128 value,bytes metadata)";

pub const RAV_TYPE_STRING: &str =
    "ReceiptAggregateVoucher(bytes32 collectionId,address payer,address serviceProvider,address dataService,uint64 timestampNs,uint128 valueAggregate,bytes metadata)";

// ── Structs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub data_service: Address,
    pub service_provider: Address,
    pub timestamp_ns: u64,
    pub nonce: u64,
    pub value: u128,
    #[serde(default)]
    pub metadata: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedReceipt {
    pub receipt: Receipt,
    /// Hex-encoded 65-byte ECDSA signature: r(32) || s(32) || v(1).
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rav {
    pub collection_id: B256,
    pub payer: Address,
    pub service_provider: Address,
    pub data_service: Address,
    pub timestamp_ns: u64,
    pub value_aggregate: u128,
    #[serde(default)]
    pub metadata: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRav {
    pub rav: Rav,
    pub signature: String,
}

/// Extract the payer (consumer) address from receipt metadata (first 20 bytes).
pub fn payer_from_metadata(metadata: &Bytes) -> Option<Address> {
    if metadata.len() >= 20 {
        Some(Address::from_slice(&metadata[..20]))
    } else {
        None
    }
}

// ── EIP-712 ───────────────────────────────────────────────────────────────────

pub fn domain_separator(name: &str, chain_id: u64, verifying_contract: Address) -> B256 {
    let type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    let encoded = (
        type_hash,
        keccak256(name.as_bytes()),
        keccak256(b"1"),
        U256::from(chain_id),
        verifying_contract,
    )
        .abi_encode();
    keccak256(&encoded)
}

pub fn eip712_hash(domain_sep: B256, receipt: &Receipt) -> B256 {
    eip712_hash_raw(domain_sep, receipt_struct_hash(receipt))
}

pub fn eip712_hash_raw(domain_sep: B256, struct_hash: B256) -> B256 {
    let mut buf = [0u8; 66];
    buf[0] = 0x19;
    buf[1] = 0x01;
    buf[2..34].copy_from_slice(domain_sep.as_slice());
    buf[34..66].copy_from_slice(struct_hash.as_slice());
    keccak256(buf)
}

pub fn recover_signer(hash: B256, sig_hex: &str) -> anyhow::Result<Address> {
    let bytes = hex::decode(sig_hex.trim_start_matches("0x"))?;
    anyhow::ensure!(bytes.len() == 65, "signature must be 65 bytes, got {}", bytes.len());
    let v = bytes[64];
    let rec_id_byte = if v >= 27 { v - 27 } else { v };
    let rec_id = RecoveryId::from_byte(rec_id_byte)
        .ok_or_else(|| anyhow::anyhow!("invalid recovery id {v}"))?;
    let sig = K256Sig::from_slice(&bytes[..64])?;
    let vk = VerifyingKey::recover_from_prehash(hash.as_slice(), &sig, rec_id)?;
    let encoded = vk.to_encoded_point(false);
    let pubkey_hash = keccak256(&encoded.as_bytes()[1..]);
    Ok(Address::from_slice(&pubkey_hash[12..]))
}

fn receipt_struct_hash(r: &Receipt) -> B256 {
    let type_hash = keccak256(RECEIPT_TYPE_STRING.as_bytes());
    let encoded = (
        type_hash,
        r.data_service,
        r.service_provider,
        r.timestamp_ns,
        r.nonce,
        r.value,
        keccak256(&r.metadata),
    )
        .abi_encode();
    keccak256(&encoded)
}

pub fn rav_struct_hash(rav: &Rav) -> B256 {
    let type_hash = keccak256(RAV_TYPE_STRING.as_bytes());
    let encoded = (
        type_hash,
        rav.collection_id,
        rav.payer,
        rav.service_provider,
        rav.data_service,
        rav.timestamp_ns,
        rav.value_aggregate,
        keccak256(&rav.metadata),
    )
        .abi_encode();
    keccak256(&encoded)
}

pub fn collection_id(payer: Address, service_provider: Address, data_service: Address) -> B256 {
    let encoded = (payer, service_provider, data_service).abi_encode();
    keccak256(&encoded)
}

// ── Validation ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum TapError {
    InvalidReceipt(String),
    ReceiptExpired,
    UnauthorizedSender(String),
}

impl std::fmt::Display for TapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TapError::InvalidReceipt(msg) => write!(f, "invalid receipt: {msg}"),
            TapError::ReceiptExpired => write!(f, "receipt expired"),
            TapError::UnauthorizedSender(s) => write!(f, "unauthorized sender: {s}"),
        }
    }
}

pub struct ValidatedReceipt {
    pub receipt: Receipt,
    pub signer: Address,
    pub payer: Address,
    pub signature: String,
}

pub fn validate_receipt(
    header_value: &str,
    domain_sep: B256,
    authorized_senders: &[Address],
    data_service: Address,
    service_provider: Address,
    max_age_ns: u64,
    now_ns: u64,
) -> Result<ValidatedReceipt, TapError> {
    let signed: SignedReceipt = serde_json::from_str(header_value)
        .map_err(|e| TapError::InvalidReceipt(e.to_string()))?;

    let r = &signed.receipt;

    if r.data_service != data_service {
        return Err(TapError::InvalidReceipt(format!(
            "data_service mismatch: expected {data_service}, got {}",
            r.data_service
        )));
    }

    if r.service_provider != service_provider {
        return Err(TapError::InvalidReceipt(format!(
            "service_provider mismatch: expected {service_provider}, got {}",
            r.service_provider
        )));
    }

    if now_ns.saturating_sub(r.timestamp_ns) > max_age_ns {
        return Err(TapError::ReceiptExpired);
    }

    let msg_hash = eip712_hash(domain_sep, r);
    let signer = recover_signer(msg_hash, &signed.signature)
        .map_err(|e| TapError::InvalidReceipt(format!("signature recovery failed: {e}")))?;

    if !authorized_senders.is_empty() && !authorized_senders.contains(&signer) {
        return Err(TapError::UnauthorizedSender(signer.to_string()));
    }

    let payer = payer_from_metadata(&r.metadata).unwrap_or(signer);

    Ok(ValidatedReceipt {
        receipt: signed.receipt,
        signer,
        payer,
        signature: signed.signature,
    })
}

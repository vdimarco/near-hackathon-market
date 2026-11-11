//! Shared sandbox helpers for the recorded replay and the fresh run.

use anyhow::{bail, Context, Result};
use borsh::BorshSerialize;
use near_workspaces::network::Sandbox;
use near_workspaces::types::{AccessKey, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: u32 = 2;
pub const BPS: u32 = 10_000;

/// The same fields, in the same order, as `Commitment` in the contract.
/// This copy is separate code, so the replay also checks the encoding.
#[derive(BorshSerialize)]
pub struct Commitment {
    protocol_version: u32,
    contract_id: String,
    case_id: u64,
    evidence_hash: String,
    policy_hash: String,
    juror: String,
    probs: Vec<u32>,
    rationale_hash: String,
    salt: String,
}

/// Offline copy of the decision rule: an option needs more than half.
pub fn strict_majority(sums: &[u32], n_reads: u32) -> Option<u32> {
    sums.iter().position(|s| *s > BPS / 2 * n_reads).map(|i| i as u32)
}

pub const JUROR_METHODS: [&str; 3] = ["commit", "reveal", "final_read"];
pub const RULINGS: [&str; 3] = ["worker wins", "redo", "requester wins"];

pub struct Panel {
    pub contract: Contract,
    pub escrow: Contract,
    pub root: Account,
    /// These two accounts never sign a transaction, so a balance change is a payment.
    pub worker: Account,
    pub buyer: Account,
    pub requester: Account,
    pub jurors: Vec<Account>,
    pub ids: Vec<String>,
}

pub fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{:02x}", b)).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn commit_hash(panel: &Panel, case_id: u64, evidence: &str, policy: &str, j: usize, probs: &[u32], rationale: &str, salt: &str) -> String {
    let c = Commitment {
        protocol_version: PROTOCOL_VERSION,
        contract_id: panel.contract.id().to_string(),
        case_id,
        evidence_hash: evidence.to_string(),
        policy_hash: policy.to_string(),
        juror: panel.ids[j].clone(),
        probs: probs.to_vec(),
        rationale_hash: rationale.to_string(),
        salt: salt.to_string(),
    };
    sha_hex(&borsh::to_vec(&c).expect("encoding cannot fail"))
}

/// Build a contract with plain cargo. No extra build tool is needed.
pub fn build_contract(package: &str) -> Result<Vec<u8>> {
    let status = std::process::Command::new("cargo")
        .args(["build", "-q", "-p", package, "--target", "wasm32-unknown-unknown", "--release"])
        // `cargo near build` sets this flag. It selects the on-chain host functions.
        .env("RUSTFLAGS", "--cfg near")
        .status()
        .context("cannot run cargo")?;
    if !status.success() {
        bail!("contract build failed. Run: rustup target add wasm32-unknown-unknown");
    }
    let build_folder = std::env::var("CARGO_TARGET_DIR").ok();
    let path = wasm_path(build_folder.as_deref(), package);
    std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))
}

/// The file that cargo writes for a contract.
/// `build_folder` is the value of `CARGO_TARGET_DIR`. Cargo writes into `target` when it is not set.
pub fn wasm_path(build_folder: Option<&str>, package: &str) -> std::path::PathBuf {
    std::path::Path::new(build_folder.unwrap_or("target"))
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(format!("{}.wasm", package.replace('-', "_")))
}

pub fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").chars().take(150).collect()
}

/// The reason that the node gives when it refuses a transaction for its key,
/// for example `RequiresFullAccess` or `MethodNameMismatch`.
pub fn key_refusal(error_text: &str) -> String {
    match error_text.split("InvalidAccessKeyError(").nth(1) {
        Some(rest) => rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect(),
        None => first_line(error_text),
    }
}

pub fn u32s(v: &Value) -> Option<Vec<u32>> {
    v.as_array()?.iter().map(|x| x.as_u64().map(|n| n as u32)).collect()
}

/// The single top option of a list of sums. `None` when there is a tie.
pub fn top(sums: &[u32]) -> Option<usize> {
    let max = *sums.iter().max()?;
    let tops: Vec<usize> = (0..sums.len()).filter(|i| sums[*i] == max).collect();
    if tops.len() == 1 { Some(tops[0]) } else { None }
}

pub fn offline_sums(reads: &[&Vec<u32>], n_options: usize) -> Vec<u32> {
    let mut sums = vec![0u32; n_options];
    for read in reads {
        for (i, p) in read.iter().enumerate() {
            sums[i] += p;
        }
    }
    sums
}

pub async fn view_case(panel: &Panel, id: u64) -> Result<Value> {
    Ok(panel.contract.view("get_case").args_json(json!({ "case_id": id })).await?.json()?)
}

pub async fn open_case(panel: &Panel, key: &str, evidence: &str, policy: &str, options: &[String], reveal_secs: u64) -> Result<u64> {
    Ok(panel
        .requester
        .call(panel.contract.id(), "open_case")
        .args_json(json!({
            "case_key": key, "evidence_hash": evidence, "policy_hash": policy, "options": options,
            "commit_secs": 600, "audit_secs": 600, "reveal_secs": reveal_secs
        }))
        .transact()
        .await?
        .into_result()?
        .json()?)
}

pub async fn setup(worker: &Worker<Sandbox>, n: usize) -> Result<Panel> {
    let wasm = build_contract("lock-jury")?;
    let escrow_wasm = build_contract("lock-escrow")?;
    let root = worker.root_account()?;
    let jury_account = root.create_subaccount("jury").initial_balance(NearToken::from_near(40)).transact().await?.into_result()?;
    let requester = root.create_subaccount("requester").initial_balance(NearToken::from_near(30)).transact().await?.into_result()?;
    let mut jurors = Vec::new();
    for j in 0..n {
        let full = jury_account
            .create_subaccount(&format!("j{}", j + 1))
            .initial_balance(NearToken::from_near(3))
            .transact()
            .await?
            .into_result()?;
        // The juror account keeps one key. It can call 3 methods on the jury
        // contract. A call with this key cannot attach NEAR.
        let limited = SecretKey::from_random(KeyType::ED25519);
        full.batch(full.id())
            .add_key(limited.public_key(), AccessKey::function_call_access(jury_account.id(), &JUROR_METHODS, None))
            .delete_key(full.secret_key().public_key())
            .transact()
            .await?
            .into_result()?;
        jurors.push(Account::from_secret_key(full.id().clone(), limited, worker));
    }
    let ids: Vec<String> = jurors.iter().map(|a| a.id().to_string()).collect();
    let contract = jury_account.deploy(&wasm).await?.into_result()?;
    contract.call("new").args_json(json!({ "jurors": ids })).transact().await?.into_result()?;
    let escrow_account = root.create_subaccount("escrow").initial_balance(NearToken::from_near(10)).transact().await?.into_result()?;
    let escrow = escrow_account.deploy(&escrow_wasm).await?.into_result()?;
    escrow.call("new").args_json(json!({ "jury": contract.id() })).transact().await?.into_result()?;
    let worker_account = root.create_subaccount("worker").initial_balance(NearToken::from_near(1)).transact().await?.into_result()?;
    let buyer = root.create_subaccount("buyer").initial_balance(NearToken::from_near(1)).transact().await?.into_result()?;
    Ok(Panel { contract, escrow, root, worker: worker_account, buyer, requester, jurors, ids })
}

/// Fund a 2 NEAR deal for a jury case. Option 0 pays the worker, option 2 refunds the buyer.
pub async fn fund(panel: &Panel, case_id: u64, worker: &str) -> Result<u64> {
    Ok(panel
        .requester
        .call(panel.escrow.id(), "fund")
        .args_json(json!({ "jury_case_id": case_id, "worker": worker, "requester": panel.buyer.id(),
            "worker_option": 0, "requester_option": 2 }))
        .deposit(NearToken::from_near(2))
        .transact()
        .await?
        .into_result()?
        .json()?)
}

/// Returns true if the escrow accepted the settle call.
pub async fn settle(panel: &Panel, deal: u64) -> Result<bool> {
    let r = panel.requester.call(panel.escrow.id(), "settle").args_json(json!({ "deal_id": deal })).max_gas().transact().await?;
    Ok(r.is_success())
}

pub async fn deal_view(panel: &Panel, deal: u64) -> Result<Value> {
    Ok(panel.escrow.view("get_deal").args_json(json!({ "deal_id": deal })).await?.json()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_wasm_file_is_in_target_when_cargo_has_no_other_build_folder() {
        assert_eq!(wasm_path(None, "lock-jury"), Path::new("target/wasm32-unknown-unknown/release/lock_jury.wasm"));
    }

    #[test]
    fn the_wasm_file_follows_cargo_target_dir() {
        assert_eq!(
            wasm_path(Some("/tmp/build"), "lock-escrow"),
            Path::new("/tmp/build/wasm32-unknown-unknown/release/lock_escrow.wasm")
        );
    }
}

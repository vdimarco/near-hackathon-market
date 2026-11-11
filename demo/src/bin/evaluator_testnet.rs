//! Live run on NEAR testnet for the evaluator bet.
//!
//! The program deploys the jury contract and the evaluator contract on NEAR
//! testnet. It replays the recorded reads of two synthetic disputes through
//! the jury, and it asks the evaluator to sign each ruling. The evaluator
//! calls the signer network (`v1.signer-prod.testnet`). The program writes the
//! signatures to `fixtures/evaluator/testnet-rulings.json`. The Foundry test
//! `evm/test/RecordedRuling.t.sol` replays that file with no key.
//!
//! The program makes no model call. The reads come from `runs/fresh-2026-11-10/`.
//! It needs no account from you: the testnet faucet funds new accounts.

use anyhow::{bail, Context, Result};
use borsh::BorshSerialize;
use demo::{build_contract, JUROR_METHODS, PROTOCOL_VERSION};
use near_workspaces::network::Testnet;
use near_workspaces::result::ExecutionSuccess;
use near_workspaces::types::{AccessKey, Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, AccountId, Worker};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::time::Duration;

/// The RPC server. Set `NEAR_TESTNET_RPC` to use another one.
const DEFAULT_RPC: &str = "https://test.rpc.fastnear.com";
/// The program waits this long before each request, so that a free RPC server accepts all of them.
const PAUSE: Duration = Duration::from_millis(400);
const SIGNER: &str = "v1.signer-prod.testnet";
const SIGNER_PATH: &str = "erc8183-1";
/// The EVM values that the digests bind. The recorded check sets a local EVM
/// to these values. 5042002 is the chain id of Arc Testnet, and the escrow
/// address is the address of the ERC-8183 reference contract there. No
/// transaction goes to Arc Testnet.
const CHAIN_ID: u64 = 5042002;
const ESCROW: &str = "0x0747eef0706327138c69792bf28cd525089e4583";
/// A fixed address for the adapter in the recorded check. The bytes spell "JURYEVMADAPTER".
const ADAPTER: &str = "0x4a55525945564d41444150544552000000000001";
/// The recorded disputes to replay, with the ERC-8183 job id of each.
const CASES: [(&str, u64); 2] = [("synthetic-dispute-02-v1", 1), ("synthetic-dispute-01-v1", 2)];
const RUN_DIR: &str = "runs/fresh-2026-11-10";
const OUT: &str = "fixtures/evaluator/testnet-rulings.json";

/// The same fields, in the same order, as `Commitment` in the jury contract.
#[derive(BorshSerialize)]
struct Commitment {
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn keccak(bytes: &[u8]) -> Vec<u8> {
    Keccak256::digest(bytes).to_vec()
}

/// Half of the order of the secp256k1 group, and the order, as two 128-bit halves.
const HALF_N: (u128, u128) = (0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF, 0x5D576E7357A4501DDFE92F46681B20A0);
const N: (u128, u128) = (0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFE, 0xBAAEDCE6AF48A03BBFD25E8CD0364141);

/// The Ethereum form (v, r, s) of an answer of the NEAR signer.
/// `big_r` is a compressed point: one prefix byte and the 32-byte x value.
/// Ethereum libraries refuse an `s` in the upper half of the group order, so
/// this function moves such an `s` to the lower half and changes `v` with it.
fn eth_signature(big_r: &str, s: &str, recovery_id: u8) -> Result<(u8, String, String)> {
    let is_hex = |t: &str| t.bytes().all(|b| b.is_ascii_hexdigit());
    if big_r.len() != 66 || !is_hex(big_r) || !(big_r.starts_with("02") || big_r.starts_with("03")) {
        bail!("big_r is not a compressed point of 33 bytes");
    }
    if s.len() != 64 || !is_hex(s) || recovery_id > 1 {
        bail!("s is not a 32-byte number, or the recovery id is not 0 or 1");
    }
    let high = u128::from_str_radix(&s[..32], 16)?;
    let low = u128::from_str_radix(&s[32..], 16)?;
    let mut v = 27 + recovery_id;
    let (mut high_out, mut low_out) = (high, low);
    if (high, low) > HALF_N {
        // s moves to (order - s). The subtraction borrows from the high half when it must.
        let (low_diff, borrow) = N.1.overflowing_sub(low);
        low_out = low_diff;
        high_out = N.0 - high - u128::from(borrow);
        v = 55 - v;
    }
    Ok((v, big_r[2..].to_lowercase(), format!("{:032x}{:032x}", high_out, low_out)))
}

/// True if the RPC server refused a request before the network saw it.
fn refused_by_rpc(error: &anyhow::Error) -> bool {
    let text = format!("{:#}", error).to_lowercase();
    text.contains("rate limit") || text.contains("too many requests") || text.contains("unable to fulfill the query request")
}

/// Gas for a plain call. A juror account holds little NEAR, and the network
/// takes the price of all attached gas before it runs a call.
const SMALL_GAS: u64 = 20;
/// Gas for `sign_ruling`: the call to the jury, the call to the signer, and two callbacks.
const SIGN_RULING_GAS: u64 = 300;

/// One contract call with `tgas` of gas. If the RPC server refuses the
/// request, the call runs again after a wait.
async fn send(who: &Account, contract: &AccountId, method: &str, args: Value, tgas: u64) -> Result<ExecutionSuccess> {
    let mut wait = Duration::from_secs(5);
    loop {
        tokio::time::sleep(PAUSE).await;
        match who.call(contract, method).args_json(args.clone()).gas(Gas::from_tgas(tgas)).transact().await {
            Ok(done) => return done.into_result().map_err(|e| anyhow::anyhow!("{} failed: {}", method, e)),
            Err(e) => {
                let e = anyhow::Error::from(e);
                if !refused_by_rpc(&e) || wait > Duration::from_secs(80) {
                    return Err(e.context(format!("{} did not run", method)));
                }
                println!("   the RPC server refused a request: wait {} s, then send {} again", wait.as_secs(), method);
                tokio::time::sleep(wait).await;
                wait *= 2;
            }
        }
    }
}

/// One view call, with the same wait rule as `send`.
async fn read(worker: &Worker<Testnet>, contract: &AccountId, method: &str, args: Value) -> Result<Value> {
    let mut wait = Duration::from_secs(5);
    loop {
        tokio::time::sleep(PAUSE).await;
        match worker.view(contract, method).args_json(args.clone()).await {
            Ok(result) => return Ok(result.json()?),
            Err(e) => {
                let e = anyhow::Error::from(e);
                if !refused_by_rpc(&e) || wait > Duration::from_secs(80) {
                    return Err(e.context(format!("{} did not run", method)));
                }
                println!("   the RPC server refused a request: wait {} s, then read {} again", wait.as_secs(), method);
                tokio::time::sleep(wait).await;
                wait *= 2;
            }
        }
    }
}

fn u32s(v: &Value) -> Result<Vec<u32>> {
    v.as_array().context("not a list")?.iter().map(|x| x.as_u64().map(|n| n as u32).context("not a number")).collect()
}

fn text<'a>(v: &'a Value, what: &str) -> Result<&'a str> {
    v.as_str().with_context(|| format!("the record has no {}", what))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("== Live run on NEAR testnet: the jury rules, and the signer network signs the ruling");
    let jury_wasm = build_contract("lock-jury")?;
    let evaluator_wasm = build_contract("lock-evaluator")?;
    let rpc = std::env::var("NEAR_TESTNET_RPC").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let worker = near_workspaces::testnet().rpc_addr(&rpc).await?;
    println!("   RPC server:         {}", rpc);

    let opener = worker.dev_create_account().await?;
    let jury = worker.dev_deploy(&jury_wasm).await?;
    let evaluator = worker.dev_deploy(&evaluator_wasm).await?;
    println!("   opener account:     {}", opener.id());
    println!("   jury contract:      {}", jury.id());
    println!("   evaluator contract: {}", evaluator.id());

    // Six juror accounts under the jury account. Each keeps one key that can
    // call three methods of the jury contract.
    let mut jurors = Vec::new();
    for j in 0..6 {
        tokio::time::sleep(PAUSE).await;
        let full = jury
            .as_account()
            .create_subaccount(&format!("j{}", j + 1))
            .initial_balance(NearToken::from_millinear(500))
            .transact()
            .await?
            .into_result()?;
        let limited = SecretKey::from_random(KeyType::ED25519);
        tokio::time::sleep(PAUSE).await;
        full.batch(full.id())
            .add_key(limited.public_key(), AccessKey::function_call_access(jury.id(), &JUROR_METHODS, None))
            .delete_key(full.secret_key().public_key())
            .transact()
            .await?
            .into_result()?;
        jurors.push(Account::from_secret_key(full.id().clone(), limited, &worker));
    }
    let ids: Vec<String> = jurors.iter().map(|a| a.id().to_string()).collect();
    send(jury.as_account(), jury.id(), "new", json!({ "jurors": ids }), SMALL_GAS).await?;
    let settings = json!({ "jury": jury.id(), "signer": SIGNER, "opener": opener.id() });
    send(evaluator.as_account(), evaluator.id(), "new", settings, SMALL_GAS).await?;

    // Delete the full access key of each contract account. After this, nobody can replace the code.
    let mut key_deletions = serde_json::Map::new();
    for (name, contract) in [("jury", &jury), ("evaluator", &evaluator)] {
        let account = contract.as_account();
        tokio::time::sleep(PAUSE).await;
        let done = account.batch(account.id()).delete_key(account.secret_key().public_key()).transact().await?.into_result()?;
        tokio::time::sleep(PAUSE).await;
        let keys = account.view_access_keys().await?;
        if !keys.is_empty() {
            bail!("the {} account still has {} access keys", name, keys.len());
        }
        println!("   {} account: full access key deleted, 0 keys left   ok", name);
        key_deletions.insert(name.to_string(), json!(done.outcome().transaction_hash.to_string()));
    }

    // The public key that the signer network derives for the evaluator account and the path.
    let key_request = json!({ "path": SIGNER_PATH, "predecessor": evaluator.id(), "domain_id": 0 });
    let derived = read(&worker, &SIGNER.parse()?, "derived_public_key", key_request).await?;
    let derived = text(&derived, "derived public key")?.to_string();
    let point = bs58::decode(derived.strip_prefix("secp256k1:").context("the derived key is not a secp256k1 key")?).into_vec()?;
    if point.len() != 64 {
        bail!("the derived key has {} bytes, and 64 are expected", point.len());
    }
    let signer_address = format!("0x{}", hex(&keccak(&point)[12..]));
    println!("   signer address of the evaluator, from the signer network: {}", signer_address);

    let mut rulings = Vec::new();
    for (file, job_id) in CASES {
        let path = format!("{}/{}.json", RUN_DIR, file);
        let record: Value = serde_json::from_str(&std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path))?)?;
        let description = text(&record["evidence"]["requirements"], "requirements")?;
        let description_hash = hex(&keccak(description.as_bytes()));
        let case_key = format!("eip155:{}:{}:{}:{}:{}", CHAIN_ID, ESCROW, job_id, ADAPTER, description_hash);
        let evidence_hash = text(&record["evidence_hash"], "evidence_hash")?;
        let policy_hash = text(&record["policy_hash"], "policy_hash")?;
        let expected = text(&record["comparison"]["contract_ruling"], "contract_ruling")?;

        let case_args = json!({ "case_key": case_key, "evidence_hash": evidence_hash, "policy_hash": policy_hash,
            "options": record["options"], "commit_secs": 3600, "audit_secs": 3600, "reveal_secs": 3600 });
        let opened = send(&opener, jury.id(), "open_case", case_args, SMALL_GAS).await?;
        let open_tx = opened.outcome().transaction_hash.to_string();
        let case_id: u64 = opened.json()?;

        for (j, juror) in jurors.iter().enumerate() {
            let first = &record["jurors"][j]["first"];
            let commitment = Commitment {
                protocol_version: PROTOCOL_VERSION,
                contract_id: jury.id().to_string(),
                case_id,
                evidence_hash: evidence_hash.to_string(),
                policy_hash: policy_hash.to_string(),
                juror: juror.id().to_string(),
                probs: u32s(&first["probs"])?,
                rationale_hash: text(&first["rationale_hash"], "rationale_hash")?.to_string(),
                salt: text(&first["salt"], "salt")?.to_string(),
            };
            let hash = hex(&Sha256::digest(borsh::to_vec(&commitment)?));
            send(juror, jury.id(), "commit", json!({ "case_id": case_id, "hash": hash }), SMALL_GAS).await?;
        }
        for (j, juror) in jurors.iter().enumerate() {
            let probs = &record["jurors"][j]["after_discussion"]["probs"];
            send(juror, jury.id(), "final_read", json!({ "case_id": case_id, "probs": probs }), SMALL_GAS).await?;
        }
        for (j, juror) in jurors.iter().enumerate() {
            let first = &record["jurors"][j]["first"];
            let reveal = json!({ "case_id": case_id, "probs": first["probs"], "rationale_hash": first["rationale_hash"], "salt": first["salt"] });
            send(juror, jury.id(), "reveal", reveal, SMALL_GAS).await?;
        }
        let case = read(&worker, jury.id(), "get_case", json!({ "case_id": case_id })).await?;
        let ruling = case["ruling"].as_u64().and_then(|r| case["options"][r as usize].as_str()).unwrap_or("no ruling");
        if case["outcome"] != "Decided" || ruling != expected {
            bail!("{}: the testnet jury gave {} / {}, and the recorded run gave {}", file, case["outcome"], ruling, expected);
        }
        println!("   {}: jury case {} on testnet is DECIDED, {}, as in the recorded run   ok", file, case_id, ruling);

        let request = json!({ "case_id": case_id, "job": { "chain_id": CHAIN_ID, "escrow": ESCROW,
            "job_id": job_id.to_string(), "adapter": ADAPTER, "description_hash": description_hash } });
        let signed = send(&opener, evaluator.id(), "sign_ruling", request, SIGN_RULING_GAS).await?;
        let sign_tx = signed.outcome().transaction_hash.to_string();
        let gas = signed.total_gas_burnt;
        let answer: String = signed.json().with_context(|| format!("sign_ruling gave no text in transaction {}", sign_tx))?;
        let stored = read(&worker, evaluator.id(), "get_ruling", json!({ "case_id": case_id })).await?;
        if answer != "signed" || stored["status"] != "Signed" {
            bail!("{}: the evaluator answered \"{}\" in transaction {}", file, answer, sign_tx);
        }
        let big_r = text(&stored["big_r"], "big_r")?;
        let s = text(&stored["s"], "s")?;
        let recovery_id = stored["recovery_id"].as_u64().context("no recovery_id")? as u8;
        let (v, r, s_low) = eth_signature(big_r, s, recovery_id)?;
        println!("   {}: the signer network signed the ruling ({} burnt)   ok", file, gas);
        rulings.push(json!({
            "source": path,
            "near": { "case_id": case_id, "case_key": case_key, "open_case_tx": open_tx, "sign_ruling_tx": sign_tx,
                "ruling": ruling, "big_r": big_r, "s": s, "recovery_id": recovery_id },
            "job_id": job_id,
            "description": description,
            "description_hash": format!("0x{}", description_hash),
            "verdict": stored["verdict"],
            "reason": format!("0x{}", text(&stored["reason"], "reason")?),
            "digest": format!("0x{}", text(&stored["digest"], "digest")?),
            "v": v,
            "r": format!("0x{}", r),
            "s": format!("0x{}", s_low),
        }));
    }

    let fixture = json!({
        "note": "Signatures from the NEAR testnet signer network for rulings of the jury contract. Written by `make evaluator-live`. The Foundry test evm/test/RecordedRuling.t.sol replays this file on a local EVM.",
        "near_network": "testnet",
        "signer_contract": SIGNER,
        "signer_path": SIGNER_PATH,
        "signer_public_key": derived,
        "signer_address": signer_address,
        "opener": opener.id(),
        "jury": jury.id(),
        "evaluator": evaluator.id(),
        "key_deletion_txs": key_deletions,
        "chain_id": CHAIN_ID,
        "escrow": ESCROW,
        "adapter": ADAPTER,
        "rulings": rulings,
    });
    std::fs::create_dir_all("fixtures/evaluator")?;
    std::fs::write(OUT, serde_json::to_string_pretty(&fixture)? + "\n")?;
    println!("   wrote {}", OUT);
    println!("LIVE RUN COMPLETE");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_with_a_low_s_keeps_its_values() {
        let got = eth_signature(
            "02c446e5054abd1b763a777c7f6fb9e37181b2a7c467372471cecee6f423270113",
            "40e1a8f8fa50c7517844746dec7fa09ea8424a66a721b30029ab4d728793414b",
            0,
        )
        .unwrap();
        assert_eq!(got.0, 27);
        assert_eq!(got.1, "c446e5054abd1b763a777c7f6fb9e37181b2a7c467372471cecee6f423270113");
        assert_eq!(got.2, "40e1a8f8fa50c7517844746dec7fa09ea8424a66a721b30029ab4d728793414b");
    }

    #[test]
    fn recovery_id_1_gives_v_28() {
        let got = eth_signature(&format!("03{}", "11".repeat(32)), &"01".repeat(32), 1).unwrap();
        assert_eq!(got.0, 28);
    }

    /// The group order minus 1 is in the upper half. Its lower form is 1, and v changes from 27 to 28.
    #[test]
    fn an_s_in_the_upper_half_moves_to_the_lower_half() {
        let got = eth_signature(
            &format!("02{}", "11".repeat(32)),
            "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364140",
            0,
        )
        .unwrap();
        assert_eq!(got.0, 28);
        assert_eq!(got.2, format!("{:064x}", 1));
    }

    /// Half of the order, rounded down, is the largest s that stays.
    #[test]
    fn the_largest_low_s_stays() {
        let half = "7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0";
        let got = eth_signature(&format!("02{}", "11".repeat(32)), half, 0).unwrap();
        assert_eq!((got.0, got.2.as_str()), (27, half));
    }

    #[test]
    fn a_malformed_point_is_refused() {
        assert!(eth_signature(&"11".repeat(33), &"01".repeat(32), 0).is_err());
        assert!(eth_signature("02", &"01".repeat(32), 0).is_err());
    }
}

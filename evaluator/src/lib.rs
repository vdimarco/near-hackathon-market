//! Evaluator for ERC-8183 jobs on EVM chains.
//!
//! This contract reads a closed case of the jury contract. For a `Decided`
//! case with the ruling "worker wins" or "requester wins" it asks the NEAR
//! signer network (Chain Signatures) to sign a digest of the ruling. The key
//! comes from the account of this contract and a fixed path. An adapter
//! contract on the EVM chain trusts the address of that key, and it settles
//! the job when someone sends it the signature.
//!
//! The contract signs one digest for each jury case. It signs nothing else.
//! It has no owner method and no way to change its settings.

use near_sdk::json_types::U128;
use near_sdk::serde::Deserialize;
use near_sdk::serde_json::json;
use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError, PromiseOrValue};

const VIEW_GAS: Gas = Gas::from_tgas(15);
/// Gas for `on_case`. It includes the gas for the signer call and for `on_signed`.
const ON_CASE_GAS: Gas = Gas::from_tgas(160);
const SIGN_GAS: Gas = Gas::from_tgas(100);
const ON_SIGNED_GAS: Gas = Gas::from_tgas(15);

/// The ruling that pays the provider of the job.
pub const WORKER_RULING: &str = "worker wins";
/// The ruling that refunds the client of the job.
pub const REQUESTER_RULING: &str = "requester wins";
/// The path of the signing key. The account of this contract and this path fix the EVM address.
pub const SIGNER_PATH: &str = "erc8183-1";
/// Domain 0 of the signer network is the secp256k1 curve, which EVM chains use.
pub const SIGNER_DOMAIN: u64 = 0;

/// The verdict "pay the provider".
pub const COMPLETE: u8 = 1;
/// The verdict "refund the client".
pub const REJECT: u8 = 2;

/// The type text of the digest. The Solidity adapter hashes the same text.
const TYPE_TEXT: &[u8] = b"LockJuryRuling(uint256 chainId,address adapter,address escrow,uint256 jobId,bytes32 descriptionHash,uint8 verdict,bytes32 reason)";

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// Bytes from lower-case hex. Upper-case letters are refused, so that one
/// value has one text form in a case key.
fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let raw = text.as_bytes();
    if raw.len() % 2 != 0 {
        return None;
    }
    raw.chunks(2).map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?)).collect()
}

/// An EVM address: `0x` and 40 lower-case hex characters.
pub fn parse_address(text: &str) -> Option<[u8; 20]> {
    unhex(text.strip_prefix("0x")?)?.try_into().ok()
}

/// A 32-byte hash: 64 lower-case hex characters.
pub fn parse_hash32(text: &str) -> Option<[u8; 32]> {
    unhex(text)?.try_into().ok()
}

fn word(number: u128) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[16..].copy_from_slice(&number.to_be_bytes());
    out
}

fn address_word(address: &[u8; 20]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(address);
    out
}

/// The digest that the NEAR signer signs. It is Keccak-256 of eight 32-byte
/// words. `scripts/ruling_digest.py` and the adapter compute the same value.
pub fn ruling_digest(
    chain_id: u64,
    adapter: &[u8; 20],
    escrow: &[u8; 20],
    job_id: u128,
    description_hash: &[u8; 32],
    verdict: u8,
    reason: &[u8; 32],
) -> [u8; 32] {
    let mut data = Vec::with_capacity(8 * 32);
    data.extend_from_slice(&env::keccak256_array(TYPE_TEXT));
    data.extend_from_slice(&word(chain_id as u128));
    data.extend_from_slice(&address_word(adapter));
    data.extend_from_slice(&address_word(escrow));
    data.extend_from_slice(&word(job_id));
    data.extend_from_slice(description_hash);
    data.extend_from_slice(&word(verdict as u128));
    data.extend_from_slice(reason);
    env::keccak256_array(&data)
}

/// The jury case key for an ERC-8183 job.
pub fn case_key(chain_id: u64, escrow: &str, job_id: u128, adapter: &str, description_hash: &str) -> String {
    format!("eip155:{chain_id}:{escrow}:{job_id}:{adapter}:{description_hash}")
}

/// SHA-256 of one line of text that names the jury case and its ruling.
pub fn reason_hash(jury: &str, case_id: u64, evidence_hash: &str, policy_hash: &str, ruling: &str) -> [u8; 32] {
    env::sha256_array(format!("lock-jury-ruling/v1|{jury}|{case_id}|{evidence_hash}|{policy_hash}|{ruling}").as_bytes())
}

/// The ERC-8183 job that a jury case is about. The jury case key must hold the same values.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRef {
    /// The EIP-155 chain id of the EVM chain.
    pub chain_id: u64,
    /// The ERC-8183 escrow contract: `0x` and 40 lower-case hex characters.
    pub escrow: String,
    pub job_id: U128,
    /// The adapter contract, which is the evaluator of the job.
    pub adapter: String,
    /// Keccak-256 of the job description: 64 lower-case hex characters.
    pub description_hash: String,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// A sign request is in progress. A second request is refused.
    Signing,
    /// The signer answered. The signature is in the record.
    Signed,
}

/// The record of one jury case. A case with no sign request has no record.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Ruling {
    pub status: Status,
    pub job: JobRef,
    /// The option name of the jury ruling.
    pub ruling: Option<String>,
    /// 1 pays the provider. 2 refunds the client.
    pub verdict: Option<u8>,
    /// The reason that goes to the escrow, in hex. See `reason_hash`.
    pub reason: Option<String>,
    /// The digest that the signer signs, in hex. See `ruling_digest`.
    pub digest: Option<String>,
    /// The answer of the signer: a compressed curve point, in hex.
    pub big_r: Option<String>,
    pub s: Option<String>,
    pub recovery_id: Option<u8>,
}

/// The fields of the jury `get_case` view that this contract uses.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct JuryCase {
    pub case_key: String,
    pub evidence_hash: String,
    pub policy_hash: String,
    pub options: Vec<String>,
    pub opener: AccountId,
    pub outcome: String,
    pub ruling: Option<u32>,
}

/// The answer of the signer network for a secp256k1 signature.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct SignAnswer {
    pub big_r: AffinePoint,
    pub s: Scalar,
    pub recovery_id: u8,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct AffinePoint {
    pub affine_point: String,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Scalar {
    pub scalar: String,
}

#[near(serializers = [json])]
pub struct Config {
    pub jury: AccountId,
    pub signer: AccountId,
    pub opener: AccountId,
    pub signer_path: String,
    pub signer_domain: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Evaluator {
    jury: AccountId,
    signer: AccountId,
    opener: AccountId,
    rulings: LookupMap<u64, Ruling>,
}

#[near]
impl Evaluator {
    /// `jury` is the jury contract. `signer` is the contract of the signer
    /// network. The contract signs only the cases that `opener` opened.
    #[init]
    pub fn new(jury: AccountId, signer: AccountId, opener: AccountId) -> Self {
        Self { jury, signer, opener, rulings: LookupMap::new(b"r") }
    }

    /// Read the jury case and ask the signer to sign its ruling for `job`.
    /// Anyone can call this. A signed ruling cannot be signed again.
    pub fn sign_ruling(&mut self, case_id: u64, job: JobRef) -> Promise {
        require!(parse_address(&job.escrow).is_some(), "escrow must be 0x and 40 lower-case hex characters");
        require!(parse_address(&job.adapter).is_some(), "adapter must be 0x and 40 lower-case hex characters");
        require!(
            parse_hash32(&job.description_hash).is_some(),
            "description_hash must be 64 lower-case hex characters"
        );
        match self.rulings.get(&case_id).map(|r| r.status) {
            Some(Status::Signed) => env::panic_str("the ruling of this case is already signed"),
            Some(Status::Signing) => env::panic_str("a sign request for this case is in progress"),
            None => {}
        }
        self.rulings.insert(
            case_id,
            Ruling {
                status: Status::Signing,
                job,
                ruling: None,
                verdict: None,
                reason: None,
                digest: None,
                big_r: None,
                s: None,
                recovery_id: None,
            },
        );
        let args = json!({ "case_id": case_id }).to_string().into_bytes();
        Promise::new(self.jury.clone())
            .function_call("get_case".to_string(), args, NearToken::from_yoctonear(0), VIEW_GAS)
            .then(Self::ext(env::current_account_id()).with_static_gas(ON_CASE_GAS).on_case(case_id))
    }

    #[private]
    pub fn on_case(
        &mut self,
        case_id: u64,
        #[callback_result] case: Result<JuryCase, PromiseError>,
    ) -> PromiseOrValue<String> {
        let job = self.rulings.get(&case_id).expect("no sign request for this case").job.clone();
        let accepted = match self.accept(&job, case) {
            Ok(accepted) => accepted,
            Err(why) => {
                // The request ends. `sign_ruling` can run again.
                self.rulings.remove(&case_id);
                return PromiseOrValue::Value(format!("refused: {why}"));
            }
        };
        let (label, verdict, case) = accepted;
        let reason = reason_hash(self.jury.as_str(), case_id, &case.evidence_hash, &case.policy_hash, &label);
        let digest = ruling_digest(
            job.chain_id,
            &parse_address(&job.adapter).expect("checked in sign_ruling"),
            &parse_address(&job.escrow).expect("checked in sign_ruling"),
            job.job_id.0,
            &parse_hash32(&job.description_hash).expect("checked in sign_ruling"),
            verdict,
            &reason,
        );
        let record = self.rulings.get_mut(&case_id).expect("no sign request for this case");
        record.ruling = Some(label);
        record.verdict = Some(verdict);
        record.reason = Some(hex(&reason));
        record.digest = Some(hex(&digest));
        let args = json!({
            "request": { "payload_v2": { "Ecdsa": hex(&digest) }, "path": SIGNER_PATH, "domain_id": SIGNER_DOMAIN }
        })
        .to_string()
        .into_bytes();
        // The signer asks for a deposit of 1 yoctoNEAR. This contract pays it.
        PromiseOrValue::Promise(
            Promise::new(self.signer.clone())
                .function_call("sign".to_string(), args, NearToken::from_yoctonear(1), SIGN_GAS)
                .then(Self::ext(env::current_account_id()).with_static_gas(ON_SIGNED_GAS).on_signed(case_id)),
        )
    }

    #[private]
    pub fn on_signed(&mut self, case_id: u64, #[callback_result] answer: Result<SignAnswer, PromiseError>) -> String {
        let Ok(answer) = answer else {
            // A transaction can succeed with no signature. The record goes away.
            self.rulings.remove(&case_id);
            return "no signature: sign_ruling can run again".to_string();
        };
        let record = self.rulings.get_mut(&case_id).expect("no sign request for this case");
        record.big_r = Some(answer.big_r.affine_point);
        record.s = Some(answer.s.scalar);
        record.recovery_id = Some(answer.recovery_id);
        record.status = Status::Signed;
        "signed".to_string()
    }

    pub fn get_ruling(&self, case_id: u64) -> Option<Ruling> {
        self.rulings.get(&case_id).cloned()
    }

    pub fn get_config(&self) -> Config {
        Config {
            jury: self.jury.clone(),
            signer: self.signer.clone(),
            opener: self.opener.clone(),
            signer_path: SIGNER_PATH.to_string(),
            signer_domain: SIGNER_DOMAIN,
        }
    }
}

impl Evaluator {
    /// The ruling name and the verdict of a case that this contract signs, or the reason for a refusal.
    fn accept(&self, job: &JobRef, case: Result<JuryCase, PromiseError>) -> Result<(String, u8, JuryCase), String> {
        let Ok(case) = case else {
            return Err("the jury contract did not answer".to_string());
        };
        if case.opener != self.opener {
            return Err(format!("the opener of the case is {}, and this contract signs only cases of {}", case.opener, self.opener));
        }
        if case.case_key != case_key(job.chain_id, &job.escrow, job.job_id.0, &job.adapter, &job.description_hash) {
            return Err("the case key does not match the job fields".to_string());
        }
        if case.outcome != "Decided" {
            return Err(format!("the jury outcome is {}", case.outcome));
        }
        let label = case.ruling.and_then(|r| case.options.get(r as usize)).cloned().unwrap_or_default();
        let verdict = match label.as_str() {
            WORKER_RULING => COMPLETE,
            REQUESTER_RULING => REJECT,
            _ => return Err(format!("the ruling is \"{label}\", and only a ruling for the worker or the requester gets a signature")),
        };
        Ok((label, verdict, case))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex32(s: &str) -> [u8; 32] {
        parse_hash32(s).unwrap()
    }

    /// The same vector is in `scripts/ruling_digest.py` and in the adapter tests in `evm/`.
    #[test]
    fn ruling_digest_matches_the_shared_test_vector() {
        let digest = ruling_digest(
            5042002,
            &parse_address("0x1111111111111111111111111111111111111111").unwrap(),
            &parse_address("0x0747eef0706327138c69792bf28cd525089e4583").unwrap(),
            7,
            &hex32("e1b7d5e6d9a59129ef3ef4eb985b329d162fa3996cc7a03ab7b7c7e7db780ad1"),
            COMPLETE,
            &[0x22; 32],
        );
        assert_eq!(hex(&digest), "5737b69b4a81013128480187cd99686dc83320a88040ba11cdd3ea0389392f41");
    }

    #[test]
    fn case_key_names_the_chain_the_escrow_the_job_the_adapter_and_the_description() {
        let key = case_key(
            5042002,
            "0x0747eef0706327138c69792bf28cd525089e4583",
            7,
            "0x1111111111111111111111111111111111111111",
            "e1b7d5e6d9a59129ef3ef4eb985b329d162fa3996cc7a03ab7b7c7e7db780ad1",
        );
        assert_eq!(
            key,
            "eip155:5042002:0x0747eef0706327138c69792bf28cd525089e4583:7:0x1111111111111111111111111111111111111111:e1b7d5e6d9a59129ef3ef4eb985b329d162fa3996cc7a03ab7b7c7e7db780ad1"
        );
    }

    /// The expected value comes from `reason_hash` in `scripts/ruling_digest.py`.
    #[test]
    fn reason_hash_matches_the_script() {
        let reason = reason_hash("jury.near", 7, &"a".repeat(64), &"b".repeat(64), "worker wins");
        assert_eq!(hex(&reason), "6000604e39ad97010624f9888f2949da8cff18bfafbb35a47820bce9f1bc78d3");
    }

    // ---- contract behaviour ----

    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{testing_env, AccountId};

    const ESCROW: &str = "0x0747eef0706327138c69792bf28cd525089e4583";
    const ADAPTER: &str = "0x1111111111111111111111111111111111111111";
    const DESCRIPTION_HASH: &str = "e1b7d5e6d9a59129ef3ef4eb985b329d162fa3996cc7a03ab7b7c7e7db780ad1";

    fn acct(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn as_caller(caller: &str) {
        testing_env!(VMContextBuilder::new()
            .current_account_id(acct("evaluator.near"))
            .predecessor_account_id(acct(caller))
            .build());
    }

    fn job() -> JobRef {
        JobRef {
            chain_id: 5042002,
            escrow: ESCROW.to_string(),
            job_id: U128(7),
            adapter: ADAPTER.to_string(),
            description_hash: DESCRIPTION_HASH.to_string(),
        }
    }

    fn jury_case(outcome: &str, ruling: Option<u32>) -> JuryCase {
        JuryCase {
            case_key: case_key(5042002, ESCROW, 7, ADAPTER, DESCRIPTION_HASH),
            evidence_hash: "a".repeat(64),
            policy_hash: "b".repeat(64),
            options: vec!["worker wins".to_string(), "redo".to_string(), "requester wins".to_string()],
            opener: acct("opener.near"),
            outcome: outcome.to_string(),
            ruling,
        }
    }

    fn answer() -> SignAnswer {
        SignAnswer {
            big_r: AffinePoint { affine_point: format!("02{}", "c4".repeat(32)) },
            s: Scalar { scalar: "40".repeat(32) },
            recovery_id: 0,
        }
    }

    /// A contract with a sign request in progress for jury case 7.
    fn requested() -> Evaluator {
        as_caller("anyone.near");
        let mut e = Evaluator::new(acct("jury.near"), acct("signer.near"), acct("opener.near"));
        let _ = e.sign_ruling(7, job());
        as_caller("evaluator.near");
        e
    }

    fn refused(result: PromiseOrValue<String>) -> String {
        match result {
            PromiseOrValue::Value(text) => text,
            PromiseOrValue::Promise(_) => panic!("the contract asked the signer"),
        }
    }

    #[test]
    fn a_ruling_for_the_worker_gets_the_complete_verdict_and_goes_to_the_signer() {
        let mut e = requested();
        let result = e.on_case(7, Ok(jury_case("Decided", Some(0))));
        assert!(matches!(result, PromiseOrValue::Promise(_)));
        let ruling = e.get_ruling(7).unwrap();
        let reason = reason_hash("jury.near", 7, &"a".repeat(64), &"b".repeat(64), "worker wins");
        let digest = ruling_digest(
            5042002,
            &parse_address(ADAPTER).unwrap(),
            &parse_address(ESCROW).unwrap(),
            7,
            &parse_hash32(DESCRIPTION_HASH).unwrap(),
            COMPLETE,
            &reason,
        );
        assert_eq!(ruling.status, Status::Signing);
        assert_eq!(ruling.verdict, Some(COMPLETE));
        assert_eq!(ruling.ruling.as_deref(), Some("worker wins"));
        assert_eq!(ruling.reason, Some(hex(&reason)));
        assert_eq!(ruling.digest, Some(hex(&digest)));
    }

    #[test]
    fn a_ruling_for_the_requester_gets_the_reject_verdict() {
        let mut e = requested();
        let result = e.on_case(7, Ok(jury_case("Decided", Some(2))));
        assert!(matches!(result, PromiseOrValue::Promise(_)));
        assert_eq!(e.get_ruling(7).unwrap().verdict, Some(REJECT));
    }

    #[test]
    fn a_redo_ruling_gets_no_signature() {
        let mut e = requested();
        let text = refused(e.on_case(7, Ok(jury_case("Decided", Some(1)))));
        assert!(text.contains("redo"), "{text}");
        assert!(e.get_ruling(7).is_none());
    }

    #[test]
    fn a_case_with_no_decision_gets_no_signature() {
        for outcome in ["Pending", "Inconclusive", "Incomplete"] {
            let mut e = requested();
            let text = refused(e.on_case(7, Ok(jury_case(outcome, None))));
            assert!(text.contains(outcome), "{text}");
            assert!(e.get_ruling(7).is_none());
        }
    }

    #[test]
    fn a_case_for_another_job_gets_no_signature() {
        let mut e = requested();
        let mut case = jury_case("Decided", Some(0));
        case.case_key = case_key(5042002, ESCROW, 8, ADAPTER, DESCRIPTION_HASH);
        let text = refused(e.on_case(7, Ok(case)));
        assert!(text.contains("case key"), "{text}");
        assert!(e.get_ruling(7).is_none());
    }

    #[test]
    fn a_case_from_another_opener_gets_no_signature() {
        let mut e = requested();
        let mut case = jury_case("Decided", Some(0));
        case.opener = acct("stranger.near");
        let text = refused(e.on_case(7, Ok(case)));
        assert!(text.contains("opener"), "{text}");
        assert!(e.get_ruling(7).is_none());
    }

    #[test]
    fn no_answer_from_the_jury_gives_no_signature() {
        let mut e = requested();
        let text = refused(e.on_case(7, Err(near_sdk::PromiseError::Failed)));
        assert!(text.contains("jury"), "{text}");
        assert!(e.get_ruling(7).is_none());
    }

    #[test]
    fn the_answer_of_the_signer_is_recorded() {
        let mut e = requested();
        let _ = e.on_case(7, Ok(jury_case("Decided", Some(0))));
        let text = e.on_signed(7, Ok(answer()));
        assert_eq!(text, "signed");
        let ruling = e.get_ruling(7).unwrap();
        assert_eq!(ruling.status, Status::Signed);
        assert_eq!(ruling.big_r, Some(format!("02{}", "c4".repeat(32))));
        assert_eq!(ruling.s, Some("40".repeat(32)));
        assert_eq!(ruling.recovery_id, Some(0));
    }

    #[test]
    fn with_no_answer_from_the_signer_the_request_can_run_again() {
        let mut e = requested();
        let _ = e.on_case(7, Ok(jury_case("Decided", Some(0))));
        let text = e.on_signed(7, Err(near_sdk::PromiseError::Failed));
        assert!(text.contains("again"), "{text}");
        assert!(e.get_ruling(7).is_none());
        as_caller("anyone.near");
        let _ = e.sign_ruling(7, job());
        assert_eq!(e.get_ruling(7).unwrap().status, Status::Signing);
    }

    #[test]
    #[should_panic(expected = "in progress")]
    fn a_second_request_while_the_first_runs_is_refused() {
        let mut e = requested();
        as_caller("anyone.near");
        let _ = e.sign_ruling(7, job());
    }

    #[test]
    #[should_panic(expected = "already signed")]
    fn a_request_for_a_signed_ruling_is_refused() {
        let mut e = requested();
        let _ = e.on_case(7, Ok(jury_case("Decided", Some(0))));
        let _ = e.on_signed(7, Ok(answer()));
        as_caller("anyone.near");
        let _ = e.sign_ruling(7, job());
    }

    #[test]
    #[should_panic(expected = "escrow must be 0x and 40 lower-case hex characters")]
    fn a_request_with_a_malformed_address_is_refused() {
        as_caller("anyone.near");
        let mut e = Evaluator::new(acct("jury.near"), acct("signer.near"), acct("opener.near"));
        let mut bad = job();
        bad.escrow = "0x0747EEf0706327138c69792bF28Cd525089e4583".to_string();
        let _ = e.sign_ruling(7, bad);
    }

    #[test]
    fn an_address_needs_the_prefix_and_40_lower_case_hex_characters() {
        assert!(parse_address("0x0747eef0706327138c69792bf28cd525089e4583").is_some());
        assert!(parse_address("0747eef0706327138c69792bf28cd525089e4583").is_none());
        assert!(parse_address("0x0747EEf0706327138c69792bF28Cd525089e4583").is_none());
        assert!(parse_address("0x0747eef0706327138c69792bf28cd525089e45").is_none());
    }
}

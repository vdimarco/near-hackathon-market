//! Lock Before You Talk: a jury contract.
//!
//! Each juror commits a hash of its first read before the contract opens the
//! discussion phase. After the discussion each juror posts an audit report,
//! and then reveals its first read. The contract makes a decision only when
//! every juror has revealed. The audit reports never change the decision.

use near_sdk::borsh::{self, BorshSerialize};
use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, PanicOnDefault};

pub const PROTOCOL_VERSION: u32 = 2;
/// All probabilities are integers in basis points. One read sums to 10,000.
pub const BPS: u32 = 10_000;
pub const MAX_OPTIONS: usize = 8;
pub const MAX_JURORS: usize = 16;
const NS: u64 = 1_000_000_000;

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Jurors commit. The discussion is closed.
    Commit,
    /// All jurors have committed. Jurors talk, then post an audit report.
    Discuss,
    /// Jurors reveal their first reads.
    Reveal,
    /// The jury process is finished. See `Outcome`.
    Closed,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The case is not closed.
    Pending,
    /// One option has more than half of the pooled first reads. See `ruling`.
    Decided,
    /// Every juror revealed, and no option has more than half.
    Inconclusive,
    /// A required commit or reveal is missing. No decision is made.
    Incomplete,
}

#[near(serializers = [borsh])]
#[derive(Clone)]
pub struct Case {
    pub case_key: String,
    pub evidence_hash: String,
    pub policy_hash: String,
    pub options: Vec<String>,
    pub opener: AccountId,
    pub phase: Phase,
    pub outcome: Outcome,
    pub ruling: Option<u32>,
    pub audit_secs: u64,
    pub reveal_secs: u64,
    /// Deadline of the current phase.
    pub deadline_ns: u64,
    pub commits: Vec<Option<String>>,
    pub audit_posted: Vec<bool>,
    pub audit: Vec<Option<Vec<u32>>>,
    pub first: Vec<Option<Vec<u32>>>,
    pub rationale_hashes: Vec<Option<String>>,
}

/// The value that a juror hashes. The encoding is Borsh. Text fields are
/// lower-case hex strings or NEAR account ids.
#[derive(BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub struct Commitment {
    pub protocol_version: u32,
    pub contract_id: String,
    pub case_id: u64,
    pub evidence_hash: String,
    pub policy_hash: String,
    pub juror: String,
    pub probs: Vec<u32>,
    pub rationale_hash: String,
    pub salt: String,
}

impl Commitment {
    /// The bytes that go into SHA-256.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("commitment encoding cannot fail")
    }
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq)]
pub struct JurorView {
    pub juror: AccountId,
    pub committed: bool,
    pub first: Option<Vec<u32>>,
    pub rationale_hash: Option<String>,
    pub audit_posted: bool,
    /// The read after the discussion. It is a report from the juror account.
    pub audit: Option<Vec<u32>>,
    /// Total variation distance between the first read and the audit read.
    pub moved_bps: Option<u32>,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq)]
pub struct CaseView {
    pub protocol_version: u32,
    pub case_id: u64,
    pub case_key: String,
    pub evidence_hash: String,
    pub policy_hash: String,
    pub options: Vec<String>,
    pub opener: AccountId,
    pub phase: Phase,
    pub outcome: Outcome,
    /// The decided option. `None` unless the outcome is `Decided`.
    pub ruling: Option<u32>,
    pub deadline_ns: u64,
    pub jurors: Vec<JurorView>,
    pub missing_commits: Vec<AccountId>,
    pub missing_reveals: Vec<AccountId>,
    pub missing_audit_reports: Vec<AccountId>,
    /// Sum of the revealed first reads for each option. The decision uses these sums.
    pub first_sums: Option<Vec<u32>>,
    /// Sum of the audit reads for each option, over the jurors that posted one.
    pub audit_sums: Option<Vec<u32>>,
    pub audit_count: u32,
    /// The option with more than half of the audit reads, if there is one.
    /// This is a comparison only. It has no effect on the outcome.
    pub audit_majority: Option<u32>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Jury {
    jurors: Vec<AccountId>,
    cases: LookupMap<u64, Case>,
    next_case_id: u64,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_read(probs: &[u32], n_options: usize) {
    require!(probs.len() == n_options, "one probability for each option");
    require!(probs.iter().all(|p| *p <= BPS), "probability above 10000");
    let sum: u32 = probs.iter().sum();
    require!(sum == BPS, "probabilities must sum to 10000");
}

fn sums(reads: &[&Vec<u32>]) -> Option<Vec<u32>> {
    let first = reads.first()?;
    let mut out = vec![0u32; first.len()];
    for read in reads {
        for (i, p) in read.iter().enumerate() {
            out[i] = out[i].checked_add(*p).expect("sum overflow");
        }
    }
    Some(out)
}

/// The option whose sum is more than half of the total, if there is one.
/// With `n` reads the rule is `sum > 5000 * n`. Exactly half is not a majority.
pub fn strict_majority(option_sums: &[u32], n_reads: u32) -> Option<u32> {
    let half = (BPS / 2).checked_mul(n_reads).expect("overflow");
    option_sums.iter().position(|s| *s > half).map(|i| i as u32)
}

#[near]
impl Jury {
    /// The panel is fixed when the contract starts. No method changes it.
    #[init]
    pub fn new(jurors: Vec<AccountId>) -> Self {
        require!(!jurors.is_empty() && jurors.len() <= MAX_JURORS, "1 to 16 jurors");
        for (i, a) in jurors.iter().enumerate() {
            require!(!jurors[..i].contains(a), "duplicate juror");
        }
        Self { jurors, cases: LookupMap::new(b"c"), next_case_id: 0 }
    }

    /// Record a case. The evidence hash, the policy hash, and the options
    /// cannot change after this call.
    #[allow(clippy::too_many_arguments)]
    pub fn open_case(
        &mut self,
        case_key: String,
        evidence_hash: String,
        policy_hash: String,
        options: Vec<String>,
        commit_secs: u64,
        audit_secs: u64,
        reveal_secs: u64,
    ) -> u64 {
        require!(!case_key.is_empty() && case_key.len() <= 256, "case_key must have 1 to 256 characters");
        require!(is_hex64(&evidence_hash), "evidence_hash must be 64 hex characters");
        require!(is_hex64(&policy_hash), "policy_hash must be 64 hex characters");
        require!(options.len() >= 2 && options.len() <= MAX_OPTIONS, "2 to 8 options");
        require!(options.iter().all(|o| !o.is_empty() && o.len() <= 64), "bad option name");
        let day = 86_400 * 30;
        require!(
            (1..=day).contains(&commit_secs) && (1..=day).contains(&audit_secs) && (1..=day).contains(&reveal_secs),
            "each phase must last 1 second to 30 days"
        );
        let n = self.jurors.len();
        let case = Case {
            case_key,
            evidence_hash,
            policy_hash,
            options,
            opener: env::predecessor_account_id(),
            phase: Phase::Commit,
            outcome: Outcome::Pending,
            ruling: None,
            audit_secs,
            reveal_secs,
            deadline_ns: env::block_timestamp() + commit_secs * NS,
            commits: vec![None; n],
            audit_posted: vec![false; n],
            audit: vec![None; n],
            first: vec![None; n],
            rationale_hashes: vec![None; n],
        };
        let id = self.next_case_id;
        self.next_case_id += 1;
        self.cases.insert(id, case);
        env::log_str(&format!("case_opened:{}", id));
        id
    }

    /// Lock a first read. `hash` is SHA-256 of the Borsh bytes of `Commitment`, in hex.
    pub fn commit(&mut self, case_id: u64, hash: String) {
        let j = self.juror_index();
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Commit, "the commit phase is over");
        require!(env::block_timestamp() <= case.deadline_ns, "commit deadline passed");
        require!(case.commits[j].is_none(), "already committed");
        require!(is_hex64(&hash), "hash must be 64 hex characters");
        case.commits[j] = Some(hash);
        if case.commits.iter().all(|c| c.is_some()) {
            case.phase = Phase::Discuss;
            case.deadline_ns = env::block_timestamp() + case.audit_secs * NS;
            env::log_str(&format!("discussion_open:{}", case_id));
        }
    }

    /// Post the read after the discussion, or `null` for "no report".
    /// Jurors post this before any first read is revealed.
    pub fn final_read(&mut self, case_id: u64, probs: Option<Vec<u32>>) {
        let j = self.juror_index();
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Discuss, "audit reports are accepted only in the discussion phase");
        require!(env::block_timestamp() <= case.deadline_ns, "audit deadline passed");
        require!(!case.audit_posted[j], "audit report already posted");
        if let Some(p) = &probs {
            check_read(p, case.options.len());
        }
        case.audit_posted[j] = true;
        case.audit[j] = probs;
        if case.audit_posted.iter().all(|x| *x) {
            case.phase = Phase::Reveal;
            case.deadline_ns = env::block_timestamp() + case.reveal_secs * NS;
            env::log_str(&format!("reveal_open:{}", case_id));
        }
    }

    /// Show the first read. The contract checks it against the commit.
    /// The last reveal closes the case with a decision.
    pub fn reveal(&mut self, case_id: u64, probs: Vec<u32>, rationale_hash: String, salt: String) {
        let j = self.juror_index();
        let juror = self.jurors[j].clone();
        let n = self.jurors.len() as u32;
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Reveal, "reveals are accepted only in the reveal phase");
        require!(env::block_timestamp() <= case.deadline_ns, "reveal deadline passed");
        require!(case.first[j].is_none(), "already revealed");
        require!(is_hex64(&salt), "salt must be 64 hex characters");
        require!(is_hex64(&rationale_hash), "rationale_hash must be 64 hex characters");
        check_read(&probs, case.options.len());
        let commitment = Commitment {
            protocol_version: PROTOCOL_VERSION,
            contract_id: env::current_account_id().to_string(),
            case_id,
            evidence_hash: case.evidence_hash.clone(),
            policy_hash: case.policy_hash.clone(),
            juror: juror.to_string(),
            probs: probs.clone(),
            rationale_hash: rationale_hash.clone(),
            salt,
        };
        let digest = hex(&env::sha256(&commitment.to_bytes()));
        require!(Some(&digest) == case.commits[j].as_ref(), "reveal does not match the commit");
        case.first[j] = Some(probs);
        case.rationale_hashes[j] = Some(rationale_hash);
        if case.first.iter().all(|r| r.is_some()) {
            let reads: Vec<&Vec<u32>> = case.first.iter().flatten().collect();
            let option_sums = sums(&reads).expect("at least one juror");
            case.ruling = strict_majority(&option_sums, n);
            case.outcome = if case.ruling.is_some() { Outcome::Decided } else { Outcome::Inconclusive };
            case.phase = Phase::Closed;
            env::log_str(&format!("closed:{}:{:?}:{:?}", case_id, case.outcome, case.ruling));
        }
    }

    /// Move a case forward after a deadline. Anyone can call this, so a case
    /// cannot stay open if the harness stops.
    /// - Commit phase with a missing commit: the case closes as `Incomplete`.
    /// - Discussion phase: the reveal phase opens. Audit reports are optional.
    /// - Reveal phase with a missing reveal: the case closes as `Incomplete`.
    pub fn advance(&mut self, case_id: u64) -> Phase {
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase != Phase::Closed, "the case is closed");
        require!(env::block_timestamp() > case.deadline_ns, "the deadline of this phase has not passed");
        match case.phase {
            Phase::Discuss => {
                case.phase = Phase::Reveal;
                case.deadline_ns = env::block_timestamp() + case.reveal_secs * NS;
                env::log_str(&format!("reveal_open:{}", case_id));
            }
            _ => {
                case.phase = Phase::Closed;
                case.outcome = Outcome::Incomplete;
                env::log_str(&format!("closed:{}:Incomplete:None", case_id));
            }
        }
        case.phase
    }

    pub fn get_jurors(&self) -> Vec<AccountId> {
        self.jurors.clone()
    }

    pub fn case_count(&self) -> u64 {
        self.next_case_id
    }

    /// The harness calls this before it shares a juror message with a peer.
    pub fn is_discussion_open(&self, case_id: u64) -> bool {
        self.cases.get(&case_id).expect("no such case").phase == Phase::Discuss
    }

    pub fn get_case(&self, case_id: u64) -> CaseView {
        let case = self.cases.get(&case_id).expect("no such case");
        let firsts: Vec<&Vec<u32>> = case.first.iter().flatten().collect();
        let audits: Vec<&Vec<u32>> = case.audit.iter().flatten().collect();
        let audit_sums = sums(&audits);
        let audit_count = audits.len() as u32;
        let missing = |present: &dyn Fn(usize) -> bool| -> Vec<AccountId> {
            (0..self.jurors.len()).filter(|j| !present(*j)).map(|j| self.jurors[j].clone()).collect()
        };
        let jurors = self
            .jurors
            .iter()
            .enumerate()
            .map(|(j, id)| JurorView {
                juror: id.clone(),
                committed: case.commits[j].is_some(),
                first: case.first[j].clone(),
                rationale_hash: case.rationale_hashes[j].clone(),
                audit_posted: case.audit_posted[j],
                audit: case.audit[j].clone(),
                moved_bps: match (&case.first[j], &case.audit[j]) {
                    (Some(a), Some(b)) => Some(a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).sum::<u32>() / 2),
                    _ => None,
                },
            })
            .collect();
        CaseView {
            protocol_version: PROTOCOL_VERSION,
            case_id,
            case_key: case.case_key.clone(),
            evidence_hash: case.evidence_hash.clone(),
            policy_hash: case.policy_hash.clone(),
            options: case.options.clone(),
            opener: case.opener.clone(),
            phase: case.phase,
            outcome: case.outcome,
            ruling: case.ruling,
            deadline_ns: case.deadline_ns,
            jurors,
            missing_commits: missing(&|j| case.commits[j].is_some()),
            missing_reveals: missing(&|j| case.first[j].is_some()),
            missing_audit_reports: missing(&|j| case.audit[j].is_some()),
            first_sums: sums(&firsts),
            audit_majority: audit_sums.as_ref().and_then(|s| strict_majority(s, audit_count)),
            audit_sums,
            audit_count,
        }
    }
}

impl Jury {
    /// The caller is the immediate caller of the contract. It must be on the panel.
    fn juror_index(&self) -> usize {
        let caller = env::predecessor_account_id();
        self.jurors.iter().position(|a| a == &caller).expect("caller is not a juror")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    const SALT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const EVID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const POLICY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const RATIONALE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const CONTRACT: &str = "jury.near";
    const J: [&str; 6] = ["j1.jury.near", "j2.jury.near", "j3.jury.near", "j4.jury.near", "j5.jury.near", "j6.jury.near"];

    fn acct(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn at(who: &str, t_secs: u64) {
        testing_env!(VMContextBuilder::new()
            .current_account_id(acct(CONTRACT))
            .predecessor_account_id(acct(who))
            .block_timestamp(t_secs * NS)
            .build());
    }

    fn commitment(case_id: u64, who: &str, probs: &[u32]) -> Commitment {
        Commitment {
            protocol_version: PROTOCOL_VERSION,
            contract_id: CONTRACT.into(),
            case_id,
            evidence_hash: EVID.into(),
            policy_hash: POLICY.into(),
            juror: who.into(),
            probs: probs.to_vec(),
            rationale_hash: RATIONALE.into(),
            salt: SALT.into(),
        }
    }

    fn hash_of(c: &Commitment) -> String {
        hex(&env::sha256(&c.to_bytes()))
    }

    fn rulings() -> Vec<String> {
        vec!["worker wins".into(), "redo".into(), "requester wins".into()]
    }

    fn new_case() -> (Jury, u64) {
        at("requester.near", 0);
        let mut jury = Jury::new(J.iter().map(|j| acct(j)).collect());
        let id = jury.open_case("test:job-1:a1:d1:v1".into(), EVID.into(), POLICY.into(), rulings(), 60, 60, 60);
        (jury, id)
    }

    fn commit_all(jury: &mut Jury, id: u64, reads: &[[u32; 3]; 6]) {
        for (j, who) in J.iter().enumerate() {
            at(who, 1);
            jury.commit(id, hash_of(&commitment(id, who, &reads[j])));
        }
    }

    fn audit_all(jury: &mut Jury, id: u64, read: Option<[u32; 3]>) {
        for who in J {
            at(who, 2);
            jury.final_read(id, read.map(|r| r.to_vec()));
        }
    }

    fn reveal(jury: &mut Jury, id: u64, who: &str, read: &[u32]) {
        at(who, 3);
        jury.reveal(id, read.to_vec(), RATIONALE.into(), SALT.into());
    }

    const SPLIT: [[u32; 3]; 6] = [
        [9000, 500, 500], [9000, 500, 500], [9000, 500, 500],
        [500, 500, 9000], [500, 500, 9000], [500, 500, 9000],
    ];
    const WORKER: [[u32; 3]; 6] = [[7000, 2000, 1000]; 6];

    #[test]
    fn commitment_test_vector() {
        // scripts/commit_hash.py must print the same value.
        at("anyone.near", 0);
        let c = commitment(7, "j1.jury.near", &[7000, 2000, 1000]);
        assert_eq!(hash_of(&c), "49cb452949724da1a78ea133ddb650af1b8cfd4ef3db6b648b565afd0214fd87");
    }

    #[test]
    fn complete_panel_decides_from_first_reads() {
        let (mut jury, id) = new_case();
        assert!(!jury.is_discussion_open(id));
        commit_all(&mut jury, id, &WORKER);
        assert!(jury.is_discussion_open(id));
        // After the discussion every juror follows one loud voice.
        audit_all(&mut jury, id, Some([1000, 1000, 8000]));
        for who in J {
            reveal(&mut jury, id, who, &WORKER[0]);
        }
        let v = jury.get_case(id);
        assert_eq!((v.phase, v.outcome, v.ruling), (Phase::Closed, Outcome::Decided, Some(0)));
        assert_eq!(v.first_sums, Some(vec![42000, 12000, 6000]));
        assert_eq!(v.audit_majority, Some(2));
        assert_eq!(v.jurors[0].moved_bps, Some(7000));
    }

    #[test]
    fn missing_reveal_gives_incomplete_and_no_ruling() {
        // With the old rule (drop the missing juror), five reveals pool to
        // 56% for the worker. The complete panel pools to 47.5%.
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &SPLIT);
        audit_all(&mut jury, id, None);
        for j in 0..5 {
            reveal(&mut jury, id, J[j], &SPLIT[j]);
        }
        assert_eq!(jury.get_case(id).phase, Phase::Reveal);
        at("anyone.near", 500);
        assert_eq!(jury.advance(id), Phase::Closed);
        let v = jury.get_case(id);
        assert_eq!((v.outcome, v.ruling), (Outcome::Incomplete, None));
        assert_eq!(v.missing_reveals, vec![acct("j6.jury.near")]);
    }

    #[test]
    fn complete_panel_without_majority_is_inconclusive() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &SPLIT);
        audit_all(&mut jury, id, None);
        for j in 0..6 {
            reveal(&mut jury, id, J[j], &SPLIT[j]);
        }
        let v = jury.get_case(id);
        assert_eq!((v.outcome, v.ruling), (Outcome::Inconclusive, None));
        assert_eq!(v.first_sums, Some(vec![28500, 3000, 28500]));
    }

    #[test]
    fn exactly_half_is_not_a_majority() {
        assert_eq!(strict_majority(&[30000, 0, 30000], 6), None);
        assert_eq!(strict_majority(&[30001, 0, 29999], 6), Some(0));
        let (mut jury, id) = new_case();
        let reads = [[5000, 0, 5000]; 6];
        commit_all(&mut jury, id, &reads);
        audit_all(&mut jury, id, None);
        for who in J {
            reveal(&mut jury, id, who, &reads[0]);
        }
        assert_eq!(jury.get_case(id).outcome, Outcome::Inconclusive);
    }

    #[test]
    fn missing_commit_gives_incomplete() {
        let (mut jury, id) = new_case();
        at(J[0], 1);
        jury.commit(id, hash_of(&commitment(id, J[0], &WORKER[0])));
        at("anyone.near", 61);
        assert_eq!(jury.advance(id), Phase::Closed);
        let v = jury.get_case(id);
        assert_eq!(v.outcome, Outcome::Incomplete);
        assert_eq!(v.missing_commits.len(), 5);
    }

    #[test]
    fn audit_deadline_opens_reveals_without_all_reports() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        at("anyone.near", 100);
        assert_eq!(jury.advance(id), Phase::Reveal);
        for who in J {
            at(who, 101);
            jury.reveal(id, WORKER[0].to_vec(), RATIONALE.into(), SALT.into());
        }
        let v = jury.get_case(id);
        assert_eq!((v.outcome, v.ruling), (Outcome::Decided, Some(0)));
        assert_eq!(v.missing_audit_reports.len(), 6);
    }

    fn tampered_reveal(change: impl Fn(&mut Commitment)) {
        let (mut jury, id) = new_case();
        for (j, who) in J.iter().enumerate() {
            at(who, 1);
            let mut c = commitment(id, who, &WORKER[j]);
            if j == 0 {
                change(&mut c);
            }
            jury.commit(id, hash_of(&c));
        }
        audit_all(&mut jury, id, None);
        reveal(&mut jury, id, J[0], &WORKER[0]);
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_for_other_evidence_fails() {
        tampered_reveal(|c| c.evidence_hash = "d".repeat(64));
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_for_other_policy_fails() {
        tampered_reveal(|c| c.policy_hash = "d".repeat(64));
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_for_other_case_fails() {
        tampered_reveal(|c| c.case_id += 1);
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_for_other_contract_fails() {
        tampered_reveal(|c| c.contract_id = "other.near".into());
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_copied_from_other_juror_fails() {
        tampered_reveal(|c| c.juror = "j2.jury.near".into());
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_with_other_salt_fails() {
        tampered_reveal(|c| c.salt = "e".repeat(64));
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_with_other_scores_fails() {
        tampered_reveal(|c| c.probs = vec![1000, 2000, 7000]);
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn commit_with_other_rationale_fails() {
        tampered_reveal(|c| c.rationale_hash = "f".repeat(64));
    }

    #[test]
    #[should_panic(expected = "already committed")]
    fn second_commit_fails() {
        let (mut jury, id) = new_case();
        at(J[0], 1);
        jury.commit(id, hash_of(&commitment(id, J[0], &WORKER[0])));
        jury.commit(id, hash_of(&commitment(id, J[0], &[1000, 2000, 7000])));
    }

    #[test]
    #[should_panic(expected = "audit report already posted")]
    fn second_audit_report_fails() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        at(J[0], 2);
        jury.final_read(id, Some(vec![7000, 2000, 1000]));
        jury.final_read(id, Some(vec![1000, 2000, 7000]));
    }

    #[test]
    #[should_panic(expected = "already revealed")]
    fn second_reveal_fails() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        audit_all(&mut jury, id, None);
        reveal(&mut jury, id, J[0], &WORKER[0]);
        reveal(&mut jury, id, J[0], &WORKER[0]);
    }

    #[test]
    #[should_panic(expected = "reveals are accepted only in the reveal phase")]
    fn reveal_in_commit_phase_fails() {
        let (mut jury, id) = new_case();
        at(J[0], 1);
        jury.commit(id, hash_of(&commitment(id, J[0], &WORKER[0])));
        jury.reveal(id, WORKER[0].to_vec(), RATIONALE.into(), SALT.into());
    }

    #[test]
    #[should_panic(expected = "reveals are accepted only in the reveal phase")]
    fn reveal_in_discussion_phase_fails() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        reveal(&mut jury, id, J[0], &WORKER[0]);
    }

    #[test]
    #[should_panic(expected = "audit reports are accepted only in the discussion phase")]
    fn audit_report_in_commit_phase_fails() {
        let (mut jury, id) = new_case();
        at(J[0], 1);
        jury.final_read(id, None);
    }

    #[test]
    #[should_panic(expected = "the commit phase is over")]
    fn commit_after_discussion_opens_fails() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        at(J[0], 2);
        jury.commit(id, hash_of(&commitment(id, J[0], &WORKER[0])));
    }

    #[test]
    #[should_panic(expected = "reveal deadline passed")]
    fn late_reveal_fails() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        audit_all(&mut jury, id, None);
        at(J[0], 500);
        jury.reveal(id, WORKER[0].to_vec(), RATIONALE.into(), SALT.into());
    }

    #[test]
    #[should_panic(expected = "the deadline of this phase has not passed")]
    fn early_advance_fails() {
        let (mut jury, id) = new_case();
        at("anyone.near", 5);
        jury.advance(id);
    }

    #[test]
    #[should_panic(expected = "the case is closed")]
    fn advance_on_closed_case_fails() {
        let (mut jury, id) = new_case();
        at("anyone.near", 61);
        jury.advance(id);
        jury.advance(id);
    }

    #[test]
    #[should_panic(expected = "caller is not a juror")]
    fn outsider_cannot_commit() {
        let (mut jury, id) = new_case();
        at("mallory.near", 1);
        jury.commit(id, "a".repeat(64));
    }

    #[test]
    #[should_panic(expected = "probabilities must sum to 10000")]
    fn read_must_sum_to_10000() {
        let (mut jury, id) = new_case();
        commit_all(&mut jury, id, &WORKER);
        at(J[0], 2);
        jury.final_read(id, Some(vec![7000, 2000, 2000]));
    }
}

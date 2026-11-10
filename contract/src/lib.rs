//! Lock Before You Talk: a jury contract.
//!
//! Each juror commits a salted hash of its first read before the discussion
//! opens. The contract pools the revealed first reads into the ruling. Each
//! juror can also post a read after the discussion, so anyone can see who moved.

use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, PanicOnDefault};

/// All probabilities are integers in basis points. One read sums to 10,000.
pub const BPS: u32 = 10_000;
pub const MAX_OPTIONS: usize = 8;
pub const MAX_JURORS: usize = 16;

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Jurors can commit. The discussion is closed.
    Commit,
    /// Commits are closed. The discussion is open and jurors can reveal.
    Open,
    /// The ruling is final.
    Ruled,
}

#[near(serializers = [borsh])]
#[derive(Clone)]
pub struct Case {
    pub job_id: String,
    pub evidence_hash: String,
    pub options: Vec<String>,
    pub default_option: Option<u32>,
    pub opener: AccountId,
    pub phase: Phase,
    pub commit_deadline_ns: u64,
    pub reveal_deadline_ns: u64,
    pub commits: Vec<Option<String>>,
    pub first: Vec<Option<Vec<u32>>>,
    pub finals: Vec<Option<Vec<u32>>>,
    pub ruling: Option<u32>,
    pub clear: bool,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq)]
pub struct JurorView {
    pub juror: AccountId,
    pub committed: bool,
    pub first: Option<Vec<u32>>,
    #[serde(rename = "final")]
    pub final_read: Option<Vec<u32>>,
    /// Total variation distance between the first and the final read, in basis points.
    pub moved_bps: Option<u32>,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq)]
pub struct CaseView {
    pub case_id: u64,
    pub job_id: String,
    pub evidence_hash: String,
    pub options: Vec<String>,
    pub default_option: Option<u32>,
    pub opener: AccountId,
    pub phase: Phase,
    pub commit_deadline_ns: u64,
    pub reveal_deadline_ns: u64,
    pub jurors: Vec<JurorView>,
    /// Mean of the revealed first reads.
    pub pooled_first: Option<Vec<u32>>,
    /// Mean of the reads that jurors posted after the discussion.
    pub pooled_final: Option<Vec<u32>>,
    /// The ruling, made from the first reads. `None` until `finalize`.
    pub ruling: Option<u32>,
    /// True if one option had more than half of the pooled first reads.
    pub clear: bool,
    /// What the ruling would be if the reads after the discussion decided.
    pub discussed_ruling: Option<u32>,
    pub discussed_clear: bool,
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

/// The text that a juror hashes: `case_id:juror:p0,p1,...:salt`.
pub fn preimage(case_id: u64, juror: &AccountId, probs: &[u32], salt: &str) -> String {
    let joined: Vec<String> = probs.iter().map(|p| p.to_string()).collect();
    format!("{}:{}:{}:{}", case_id, juror, joined.join(","), salt)
}

fn check_read(probs: &[u32], n_options: usize) {
    require!(probs.len() == n_options, "one probability for each option");
    let sum: u32 = probs.iter().sum();
    require!(sum == BPS, "probabilities must sum to 10000");
}

/// Mean of the reads, and the decision. The decision is the top option when it
/// has more than half of the pooled probability. If not, it is `default_option`.
fn pool(reads: &[&Vec<u32>], default_option: Option<u32>) -> (Option<Vec<u32>>, Option<u32>, bool) {
    if reads.is_empty() {
        return (None, default_option, false);
    }
    let n = reads.len() as u32;
    let mut sums = vec![0u32; reads[0].len()];
    for read in reads {
        for (i, p) in read.iter().enumerate() {
            sums[i] += p;
        }
    }
    let mut top = 0usize;
    for i in 1..sums.len() {
        if sums[i] > sums[top] {
            top = i;
        }
    }
    let clear = sums[top] * 2 > BPS * n;
    let decision = if clear { Some(top as u32) } else { default_option };
    (Some(sums.iter().map(|s| s / n).collect()), decision, clear)
}

#[near]
impl Jury {
    /// The panel is fixed when the contract starts.
    #[init]
    pub fn new(jurors: Vec<AccountId>) -> Self {
        require!(!jurors.is_empty() && jurors.len() <= MAX_JURORS, "1 to 16 jurors");
        for (i, a) in jurors.iter().enumerate() {
            require!(!jurors[..i].contains(a), "duplicate juror");
        }
        Self { jurors, cases: LookupMap::new(b"c"), next_case_id: 0 }
    }

    /// Record a dispute. `default_option` is the ruling when no option is clear.
    pub fn open_case(
        &mut self,
        job_id: String,
        evidence_hash: String,
        options: Vec<String>,
        default_option: Option<u32>,
        commit_secs: u64,
        reveal_secs: u64,
    ) -> u64 {
        require!(job_id.len() <= 128, "job_id too long");
        require!(is_hex64(&evidence_hash), "evidence_hash must be 64 hex characters");
        require!(options.len() >= 2 && options.len() <= MAX_OPTIONS, "2 to 8 options");
        require!(options.iter().all(|o| !o.is_empty() && o.len() <= 64), "bad option name");
        if let Some(d) = default_option {
            require!((d as usize) < options.len(), "default_option out of range");
        }
        require!(commit_secs > 0 && reveal_secs > 0, "deadlines must be positive");
        let now = env::block_timestamp();
        let commit_deadline_ns = now + commit_secs * 1_000_000_000;
        let n = self.jurors.len();
        let case = Case {
            job_id,
            evidence_hash,
            options,
            default_option,
            opener: env::predecessor_account_id(),
            phase: Phase::Commit,
            commit_deadline_ns,
            reveal_deadline_ns: commit_deadline_ns + reveal_secs * 1_000_000_000,
            commits: vec![None; n],
            first: vec![None; n],
            finals: vec![None; n],
            ruling: None,
            clear: false,
        };
        let id = self.next_case_id;
        self.next_case_id += 1;
        self.cases.insert(id, case);
        env::log_str(&format!("case_opened:{}", id));
        id
    }

    /// Lock a first read. `hash` is sha256 of `case_id:juror:p0,p1,...:salt` in hex.
    pub fn commit(&mut self, case_id: u64, hash: String) {
        let j = self.juror_index();
        let n = self.jurors.len();
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Commit, "commits are closed");
        require!(env::block_timestamp() <= case.commit_deadline_ns, "commit deadline passed");
        require!(case.commits[j].is_none(), "already committed");
        require!(is_hex64(&hash), "hash must be 64 hex characters");
        case.commits[j] = Some(hash);
        if case.commits.iter().filter(|c| c.is_some()).count() == n {
            case.phase = Phase::Open;
            env::log_str(&format!("discussion_open:{}", case_id));
        }
    }

    /// Close commits after the deadline when some jurors did not commit.
    pub fn close_commits(&mut self, case_id: u64) {
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Commit, "commits are closed");
        require!(env::block_timestamp() > case.commit_deadline_ns, "commit deadline not passed");
        case.phase = Phase::Open;
        env::log_str(&format!("discussion_open:{}", case_id));
    }

    /// Show the first read. The contract checks it against the commit.
    pub fn reveal(&mut self, case_id: u64, probs: Vec<u32>, salt: String) {
        let j = self.juror_index();
        let juror = self.jurors[j].clone();
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Open, "reveal needs closed commits and no ruling");
        require!(env::block_timestamp() <= case.reveal_deadline_ns, "reveal deadline passed");
        require!(case.first[j].is_none(), "already revealed");
        let commit = case.commits[j].clone().expect("no commit from this juror");
        require!(is_hex64(&salt), "salt must be 64 hex characters");
        check_read(&probs, case.options.len());
        let digest = hex(&env::sha256(preimage(case_id, &juror, &probs, &salt).as_bytes()));
        require!(digest == commit, "reveal does not match the commit");
        case.first[j] = Some(probs);
    }

    /// Post the read after the discussion. It does not change the ruling.
    pub fn final_read(&mut self, case_id: u64, probs: Vec<u32>) {
        let j = self.juror_index();
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase != Phase::Commit, "commits are still open");
        require!(case.first[j].is_some(), "reveal the first read before the final read");
        require!(case.finals[j].is_none(), "final read already posted");
        check_read(&probs, case.options.len());
        case.finals[j] = Some(probs);
    }

    /// Make the ruling from the revealed first reads. Anyone can call this when
    /// all committed jurors have revealed, or after the reveal deadline.
    pub fn finalize(&mut self, case_id: u64) -> Option<u32> {
        let case = self.cases.get_mut(&case_id).expect("no such case");
        require!(case.phase == Phase::Open, "case is not open");
        let committed = case.commits.iter().filter(|c| c.is_some()).count();
        let revealed = case.first.iter().filter(|c| c.is_some()).count();
        require!(
            revealed == committed || env::block_timestamp() > case.reveal_deadline_ns,
            "some jurors have not revealed and the deadline has not passed"
        );
        let reads: Vec<&Vec<u32>> = case.first.iter().flatten().collect();
        let (_, ruling, clear) = pool(&reads, case.default_option);
        case.ruling = ruling;
        case.clear = clear;
        case.phase = Phase::Ruled;
        env::log_str(&format!("ruled:{}:{:?}:clear={}", case_id, ruling, clear));
        ruling
    }

    pub fn get_jurors(&self) -> Vec<AccountId> {
        self.jurors.clone()
    }

    pub fn case_count(&self) -> u64 {
        self.next_case_id
    }

    /// The harness calls this before it sends a discussion prompt.
    pub fn is_discussion_open(&self, case_id: u64) -> bool {
        self.cases.get(&case_id).expect("no such case").phase != Phase::Commit
    }

    pub fn get_case(&self, case_id: u64) -> CaseView {
        let case = self.cases.get(&case_id).expect("no such case");
        let firsts: Vec<&Vec<u32>> = case.first.iter().flatten().collect();
        let finals: Vec<&Vec<u32>> = case.finals.iter().flatten().collect();
        let (pooled_first, _, _) = pool(&firsts, case.default_option);
        let (pooled_final, discussed_ruling, discussed_clear) = pool(&finals, case.default_option);
        let jurors = self
            .jurors
            .iter()
            .enumerate()
            .map(|(j, id)| {
                let moved_bps = match (&case.first[j], &case.finals[j]) {
                    (Some(a), Some(b)) => {
                        Some(a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).sum::<u32>() / 2)
                    }
                    _ => None,
                };
                JurorView {
                    juror: id.clone(),
                    committed: case.commits[j].is_some(),
                    first: case.first[j].clone(),
                    final_read: case.finals[j].clone(),
                    moved_bps,
                }
            })
            .collect();
        CaseView {
            case_id,
            job_id: case.job_id.clone(),
            evidence_hash: case.evidence_hash.clone(),
            options: case.options.clone(),
            default_option: case.default_option,
            opener: case.opener.clone(),
            phase: case.phase,
            commit_deadline_ns: case.commit_deadline_ns,
            reveal_deadline_ns: case.reveal_deadline_ns,
            jurors,
            pooled_first,
            pooled_final,
            ruling: case.ruling,
            clear: case.clear,
            discussed_ruling: if finals.is_empty() { None } else { discussed_ruling },
            discussed_clear,
        }
    }
}

impl Jury {
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
    const SEC: u64 = 1_000_000_000;

    fn acct(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn at(who: &str, t_ns: u64) {
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(acct(who))
            .block_timestamp(t_ns)
            .build());
    }

    fn jurors() -> Vec<AccountId> {
        vec![acct("j1.jury.near"), acct("j2.jury.near"), acct("j3.jury.near")]
    }

    fn commit_hash(case_id: u64, who: &str, probs: &[u32]) -> String {
        hex(&env::sha256(preimage(case_id, &acct(who), probs, SALT).as_bytes()))
    }

    fn rulings() -> Vec<String> {
        vec!["worker wins".into(), "redo".into(), "requester wins".into()]
    }

    fn open(jury: &mut Jury) -> u64 {
        at("requester.near", 0);
        jury.open_case("job-1".into(), EVID.into(), rulings(), Some(1), 60, 60)
    }

    fn commit_all(jury: &mut Jury, id: u64, reads: &[[u32; 3]; 3]) {
        for (j, who) in ["j1.jury.near", "j2.jury.near", "j3.jury.near"].iter().enumerate() {
            at(who, SEC);
            let h = commit_hash(id, who, &reads[j]);
            jury.commit(id, h);
        }
    }

    #[test]
    fn full_case_rules_from_first_reads() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        assert!(!jury.is_discussion_open(id));
        let reads = [[7000, 2000, 1000], [6000, 3000, 1000], [8000, 1000, 1000]];
        commit_all(&mut jury, id, &reads);
        assert!(jury.is_discussion_open(id));
        for (j, who) in ["j1.jury.near", "j2.jury.near", "j3.jury.near"].iter().enumerate() {
            at(who, 2 * SEC);
            jury.reveal(id, reads[j].to_vec(), SALT.into());
            // After the discussion every juror follows one loud voice.
            jury.final_read(id, vec![1000, 1000, 8000]);
        }
        at("anyone.near", 3 * SEC);
        assert_eq!(jury.finalize(id), Some(0));
        let v = jury.get_case(id);
        assert_eq!(v.phase, Phase::Ruled);
        assert_eq!(v.pooled_first, Some(vec![7000, 2000, 1000]));
        assert_eq!(v.ruling, Some(0));
        assert!(v.clear);
        assert_eq!(v.discussed_ruling, Some(2));
        assert_eq!(v.jurors[0].moved_bps, Some(7000));
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn reveal_with_changed_read_fails() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let reads = [[7000, 2000, 1000]; 3];
        commit_all(&mut jury, id, &reads);
        at("j1.jury.near", 2 * SEC);
        jury.reveal(id, vec![1000, 2000, 7000], SALT.into());
    }

    #[test]
    #[should_panic(expected = "reveal does not match the commit")]
    fn juror_cannot_copy_a_commit_from_another_juror() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let read = [7000, 2000, 1000];
        at("j1.jury.near", SEC);
        let h = commit_hash(id, "j1.jury.near", &read);
        jury.commit(id, h.clone());
        at("j2.jury.near", SEC);
        jury.commit(id, h);
        at("j3.jury.near", SEC);
        jury.commit(id, commit_hash(id, "j3.jury.near", &read));
        at("j2.jury.near", 2 * SEC);
        jury.reveal(id, read.to_vec(), SALT.into());
    }

    #[test]
    #[should_panic(expected = "reveal needs closed commits")]
    fn reveal_before_commits_close_fails() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let read = [7000, 2000, 1000];
        at("j1.jury.near", SEC);
        jury.commit(id, commit_hash(id, "j1.jury.near", &read));
        jury.reveal(id, read.to_vec(), SALT.into());
    }

    #[test]
    #[should_panic(expected = "commits are closed")]
    fn commit_after_discussion_opens_fails() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        at("j1.jury.near", SEC);
        jury.commit(id, commit_hash(id, "j1.jury.near", &[7000, 2000, 1000]));
        at("anyone.near", 61 * SEC);
        jury.close_commits(id);
        at("j2.jury.near", 62 * SEC);
        jury.commit(id, commit_hash(id, "j2.jury.near", &[7000, 2000, 1000]));
    }

    #[test]
    #[should_panic(expected = "caller is not a juror")]
    fn outsider_cannot_commit() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        at("mallory.near", SEC);
        jury.commit(id, commit_hash(id, "mallory.near", &[7000, 2000, 1000]));
    }

    #[test]
    #[should_panic(expected = "probabilities must sum to 10000")]
    fn read_must_sum_to_one() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let bad = [7000, 2000, 2000];
        commit_all(&mut jury, id, &[bad; 3]);
        at("j1.jury.near", 2 * SEC);
        jury.reveal(id, bad.to_vec(), SALT.into());
    }

    #[test]
    #[should_panic(expected = "some jurors have not revealed")]
    fn finalize_waits_for_reveals() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let reads = [[7000, 2000, 1000]; 3];
        commit_all(&mut jury, id, &reads);
        at("j1.jury.near", 2 * SEC);
        jury.reveal(id, reads[0].to_vec(), SALT.into());
        at("anyone.near", 3 * SEC);
        jury.finalize(id);
    }

    #[test]
    fn juror_that_does_not_reveal_is_left_out() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let reads = [[1000, 1000, 8000], [9000, 500, 500], [8000, 1000, 1000]];
        commit_all(&mut jury, id, &reads);
        for (j, who) in [(1, "j2.jury.near"), (2, "j3.jury.near")] {
            at(who, 2 * SEC);
            jury.reveal(id, reads[j].to_vec(), SALT.into());
        }
        at("anyone.near", 200 * SEC);
        assert_eq!(jury.finalize(id), Some(0));
        let v = jury.get_case(id);
        assert_eq!(v.pooled_first, Some(vec![8500, 750, 750]));
        assert!(v.jurors[0].committed);
        assert_eq!(v.jurors[0].first, None);
    }

    #[test]
    fn no_clear_option_gives_the_default_ruling() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        let reads = [[5000, 0, 5000], [4000, 2000, 4000], [5000, 1000, 4000]];
        commit_all(&mut jury, id, &reads);
        for (j, who) in ["j1.jury.near", "j2.jury.near", "j3.jury.near"].iter().enumerate() {
            at(who, 2 * SEC);
            jury.reveal(id, reads[j].to_vec(), SALT.into());
        }
        at("anyone.near", 3 * SEC);
        assert_eq!(jury.finalize(id), Some(1));
        assert!(!jury.get_case(id).clear);
    }

    #[test]
    #[should_panic(expected = "reveal the first read before the final read")]
    fn final_read_needs_a_reveal() {
        let mut jury = Jury::new(jurors());
        let id = open(&mut jury);
        commit_all(&mut jury, id, &[[7000, 2000, 1000]; 3]);
        at("j1.jury.near", 2 * SEC);
        jury.final_read(id, vec![7000, 2000, 1000]);
    }
}

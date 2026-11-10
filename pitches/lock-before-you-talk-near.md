# Lock Before You Talk: a jury for NEAR Agent Market

**Appetite:** Four sessions. Tuesday to Thursday for the build, Friday for the README, the demo video, and the X post. Saturday is spare time before the 18:59 Montreal deadline.

**Tracks:** Agent Economy, AI Swarms, AI Security & Defense.

## Problem

On market.near.ai an agent delivers a job and waits. If the requester does not review the work in 24 hours, the market opens a dispute. A resolver then rules, and the escrow follows the ruling. Public job pages show rulings such as "Redo" and "Requester wins".

The market's guide describes the resolver as one role, "when configured". One job page shows the same account as the requester, the dispute opener, and the resolver, and that account received the refund. A worker agent cannot check how a ruling was made.

A panel of AI jurors is the natural fix, but your Apart data shows its flaw. After free discussion, error correlation between model pairs went from 0.42 to 0.77. Six jurors became 1.2 to 1.8 effective jurors. A worker can also write its submission to persuade the first juror that speaks.

## Solution

A jury that rules on Agent Market disputes. Each juror locks its first read on NEAR before the jurors talk. The locked reads make the ruling.

1. **Open.** `open_case(job_id, evidence_hash)` records a dispute. The evidence is the job description, the submission, and the messages, read from the market API.
2. **Commit.** Six juror agents each have a sub-account (`j1.jury.testnet`). Each sends `commit(case_id, hash)`. The hash covers its probabilities for the rulings "worker wins", "redo", and "requester wins", with a 32-byte salt. Each juror signs with a function call key that can call only `commit`, `reveal`, and `final_read`.
3. **Talk.** The contract closes commits. Then two rounds of discussion run off-chain, as in the current harness.
4. **Reveal.** Each juror reveals. The contract checks the hash and pools the first reads into the ruling.
5. **Audit.** Each juror posts its estimate after the discussion. The contract stores it next to the first read. Anyone can see which jurors moved.
6. **Pay.** In the sandbox, a small escrow contract holds the job payment and follows the ruling. This shows how the market's escrow can connect.

One page shows a case: the job, the ruling, and the movement of each juror.

**Demo.** `make demo` starts a local NEAR sandbox and replays recorded jury runs on real disputed jobs from market.near.ai, next to the rulings the market gave. It needs no API key, so the AI judges can run it.

**Market use cases, in build order:**

1. **Re-judge past disputes.** The demo. It needs read access only.
2. **Review the 24-hour backlog.** The jury gives a first ruling on submissions that nobody reviewed. Same code as 1, on open jobs.
3. **A paid second opinion.** Register the jury on the market as a service. A requester or a worker sends a job ID and gets a ruling with its on-chain proof. Cut this first if time is short.
4. **Competition judge.** Competition jobs already have a judge that calls `resolve`. The jury can fill that role. Out of scope for this cycle.

## Rabbit holes

- **Market data.** The API needs a key from agent registration, and the site renders with JavaScript. On Tuesday, register an agent and pull 20 disputed jobs with their submissions. If submissions are not readable, write ten disputes in the market's job format and say so in the README.
- **No ground truth.** Real disputes have no known answer. The demo reports agreement with the market's rulings and the movement of jurors. It does not claim accuracy. The accuracy evidence stays with the Apart whodunit data.
- **Decimal math in the contract.** Probabilities are integers in basis points. The contract checks that they sum to 10,000.
- **A juror does not reveal.** After the reveal deadline the contract leaves it out of the pool and records that.
- **No clear ruling.** If no ruling has more than 50% of the pooled probability, the ruling is "redo".
- **Model access.** The harness calls Adaption today. The Tuesday session tests NEAR AI Cloud with two model families for one hour. If the test fails, the live run stays on Adaption and the README says so.
- **Contract language.** Rust with `near-sdk` and sandbox tests. Confirm the current SDK version on Tuesday before you write code.
- **Limits of the proof.** The contract proves that each commit came before the discussion that the harness ran. It cannot prove that jurors did not talk on a different channel. The README states this limit.

## No-gos

- No rulings on live market disputes. The jury is not the market's configured resolver.
- No change to the market's escrow.
- No juror staking, no slashing, and no appeals.
- No mainnet deployment.
- No NEAR Intents and no TEE attestation.
- No discussion text on-chain.
- No second page in the viewer.

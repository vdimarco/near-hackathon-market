# Lock Before You Talk: a jury on NEAR

Built for NEAR HACKATHON: Build Beyond Boundaries. Tracks: Agent Economy, AI Swarms, AI Security & Defense.

**Status on 11 November 2026:** the jury contract, the sandbox escrow, their tests, and the recorded replay work. The fresh-run harness works: six jurors read the ten synthetic disputes through the contract on 10 November with models on Adaption, and on 11 November with models on NEAR AI Cloud. The one-page viewer shows both runs. On 11 November (UTC) a third contract on NEAR testnet signed two jury rulings through Chain Signatures, and a Solidity adapter settles ERC-8183 jobs with those signatures in a recorded check on a local EVM. No juror has read a real market dispute yet. See "Not built".

## What it does

Lock Before You Talk records how an AI jury reaches a decision about disputed agent work. Six jurors assess the same evidence separately and commit their first scores on NEAR. The contract checks all six reveals and applies a fixed decision rule. Each juror also reports its scores after the discussion, so anyone can compare the two.

## The problem

NEAR Agent Market resolves disputes with an automated resolver, and an operator takes the uncertain cases. This project adds one thing to a process of that kind: a record of the first opinion of each juror, fixed before the jurors talk.

That record matters for a panel. In an exploratory study I ran at the Apart Research hackathon ([apart-swarm-alignment](https://github.com/vdimarco/apart-swarm-alignment)), measured error correlation between model pairs went from 0.42 to 0.77 after free discussion. The estimate used five model pairs, and its 95% interval for the change is -0.003 to +0.792, so it includes no change. In that study the "lock" was a sentence in the prompt. This contract makes the first read a record that nobody can change later.

## How it works

A case has four phases. The contract moves the phase. A juror cannot.

| Phase | What jurors do | How it ends |
|---|---|---|
| Commit | `commit(case_id, hash)` | The sixth commit opens the discussion. |
| Discuss | Talk off-chain, then `final_read(case_id, probs or null)` | The sixth audit report, or the deadline, opens the reveals. |
| Reveal | `reveal(case_id, probs, rationale_hash, salt)` | The sixth valid reveal closes the case. |
| Closed | Nothing | |

`advance(case_id)` lets anyone move a case after a deadline, so a case cannot stay open if the harness stops.

**The commitment.** The hash is SHA-256 of the Borsh bytes of: protocol version, contract account, case id, evidence hash, policy hash, juror account, scores, rationale hash, salt. A commit made for other evidence, another case, another contract, or another juror does not verify. `scripts/commit_hash.py` and the contract share a test vector.

**The outcomes.**

| Condition | Outcome |
|---|---|
| All six reveal, and one option has more than 50% of the pooled first reads | `Decided`, with the ruling |
| All six reveal, and no option has more than 50% | `Inconclusive` |
| A commit or a reveal is missing at the deadline | `Incomplete` |

The rule compares integer sums: `option_sum > 5000 * juror_count`. Exactly 50% is not a majority.

**Why a missing reveal stops the case.** If the contract dropped a juror that did not reveal, that juror could choose the result after the discussion. Example: three jurors give "worker wins" 90% and three give "requester wins" 90%. The full panel pools to 47.5% each, which is `Inconclusive`. If one requester-side juror withholds, the other five pool to 56% for the worker. The contract returns `Incomplete` and names the missing account. The cost is that one juror can stop a case. A design with staking can remove that cost. This one has no staking.

**The audit reports.** A juror posts its read after the discussion before any first read is revealed. The report can be `null`. Audit reports never change the outcome.

## The sandbox escrow

`escrow/` is a second contract. It shows payment that the jury outcome controls. It has no connection to the payment system of NEAR Agent Market.

- `fund` holds the attached NEAR for one jury case. It fixes the worker, the requester, and the amount.
- `settle` reads the jury case with a cross-contract call. It pays the worker or refunds the requester only on a `Decided` outcome for that party. On `Inconclusive`, `Incomplete`, a "redo" ruling, or an open case, the funds stay held.
- The jury outcome and the payment are separate records. If a transfer fails, the deal returns to `Held`, and `settle` can run again. A paid deal refuses a second `settle`.
- Held funds have no exit in this demo. A real service needs a policy for them.

## Rulings for escrows on other chains

`evaluator/` is a third NEAR contract, and `evm/` holds a Solidity adapter. Together they let a ruling of the jury settle a job on an EVM chain. The job follows [ERC-8183](https://eips.ethereum.org/EIPS/eip-8183), a draft standard for escrowed agent jobs. An ERC-8183 job names one evaluator address, and only that address can pay the provider or refund the client.

1. The client names the adapter as the evaluator of the job.
2. The jury case key holds the chain id, the escrow contract, the job id, the adapter, and the hash of the job description.
3. The jury rules. No method of the jury contract changed for this.
4. Anyone calls `sign_ruling` on `evaluator/`. It reads the closed case with `get_case`. For a `Decided` case with the ruling "worker wins" or "requester wins" it builds a 32-byte digest and asks the NEAR signer network to sign it. The signer contract is `v1.signer-prod.testnet`. The key belongs to the account of `evaluator/` and the path `erc8183-1`, and no person holds it.
5. Anyone sends the signature to `settle` on the adapter. The adapter builds the same digest from the job on its own chain and checks the signer. Then it calls `complete` or `reject` on the escrow.

The digest binds the chain id, the adapter, the escrow contract, the job id, the description hash, the verdict, and a reason. The reason is a hash of the jury account, the case id, the evidence hash, and the policy hash. Three programs compute the digest and share one test vector: `scripts/ruling_digest.py`, `evaluator/src/lib.rs`, and `evm/src/LockJuryEvaluator.sol`.

**The live run on 11 November 2026 (UTC).** `make evaluator-live` put the jury and `evaluator/` on NEAR testnet and deleted the full access key of each contract account. It replayed the recorded reads of synthetic disputes 02 and 01 from `runs/fresh-2026-11-10/`. The testnet jury gave the rulings of the recorded run: "worker wins" and "requester wins". The signer network signed each ruling, and each `sign_ruling` call burnt 18.6 Tgas.

| Record | Value |
|---|---|
| Jury contract | `dev-20261111042208-14186395012630.testnet` |
| Evaluator contract | `dev-20261111042213-22311744102799.testnet` |
| Signer address of the evaluator | `0x25a45bb5ae64a52d940e28ac510706d9f3cb7fb0` |
| `sign_ruling` for "worker wins" | [8sYBBt...GEk48](https://testnet.nearblocks.io/txns/8sYBBtB3APXufNL4pA3p73Sbaw6wo7txxR6DHNpGEk48) |
| `sign_ruling` for "requester wins" | [6xEw5S...ca6PF](https://testnet.nearblocks.io/txns/6xEw5SePD5pDiBov3xsXvXZ1ozNB3CZ4iiCfvJkca6PF) |

`fixtures/evaluator/testnet-rulings.json` holds the signatures and every value of the run. Testnet can remove old history, so that file is the record.

**The recorded check.** `make evaluator-check` replays the two signatures against the adapter and the ERC-8183 reference contract on a local EVM. Each digest binds a chain id, an escrow address, and an adapter address, so the test sets the local EVM to those values: the chain id of Arc Testnet and the address of the reference contract there. No transaction went to Arc Testnet. The signer address in the file comes from the `derived_public_key` view of the signer contract, and each signature recovers to that address.

## What the public market data shows

`scripts/fetch_market_cases.py` read 259 public job pages of the legacy market (market-legacy.near.ai) on 10 November 2026. `cases/market-legacy/index.json` has the result.

| Count | Value |
|---|---|
| Disputed jobs found | 158 |
| Market rulings | 100 requester wins, 36 redo, 21 worker wins, 1 split |
| Dispute reason "auto: submission not reviewed within 24 hours" | 113 |
| Dispute reason "Worker ghosted" | 43 |
| Pages that name a resolver | 139 |
| Pages where the resolver is also the job creator | 137 |

Read these numbers with care. The job ids come from keyword searches, so the sample is not complete and not random. The pages come from the legacy market, and the current market has different rules. One account as creator and resolver shows overlapping roles. It does not show an unfair ruling.

No page shows the submitted work. The API gives the submission only to the buyer, the worker, and the resolver. So these 158 records are partial public cases. A jury cannot rule on them, and this project does not try.

## NEAR tools used

- **Three Rust contracts** with `near-sdk` 5.30: the jury (`contract/`), the escrow (`escrow/`), and the evaluator (`evaluator/`).
- **Cross-contract calls with callbacks** from the escrow to the jury, and from the evaluator to the jury and to the signer contract.
- **Chain Signatures.** The evaluator asks `v1.signer-prod.testnet` for a secp256k1 signature. The key comes from the contract account, so an EVM contract can check a ruling with `ecrecover`.
- **Sub-accounts.** The jurors are `j1.jury.<root>` to `j6.jury.<root>`.
- **Function call keys.** Each juror account keeps one key. The key names the jury contract as receiver and lists `commit`, `reveal`, and `final_read`. A call with this key cannot attach NEAR. The demo deletes the full access key of each juror. The contract finds the caller with `predecessor_account_id()` and checks it against the panel.
- **NEAR sandbox** through `near-workspaces` 0.24 (`demo/`).

## Run it

You need Rust (rustup) and Python 3. The file `rust-toolchain.toml` selects Rust 1.97.0 and the `wasm32-unknown-unknown` target. You do not need an API key, a wallet, or a NEAR account.

### Step 1. Run the contract tests

```bash
make test
```

Expected result:

```
test result: ok. 27 passed; 0 failed
test result: ok. 3 passed; 0 failed
49cb452949724da1a78ea133ddb650af1b8cfd4ef3db6b648b565afd0214fd87
test vector ok: this script and the contract agree
```

### Step 2. Run the attack tests, the escrow tests, and 3 recorded panels

```bash
make demo-quick
```

The first build downloads the NEAR sandbox and takes a few minutes. Expected result:

```
== 2. Juror key limits
   juror key sends 1 NEAR to another account: refused                 ok
   juror key calls open_case: refused                                 ok
== 3. Attack tests on synthetic cases (fixture type: synthetic, expected outcome stated)
   split panel, all 6 reveal: INCONCLUSIVE, no ruling                 ok
   offline: a pool of the 5 reveals would say 'worker wins' (56%)     ok
   advance before the reveal deadline: refused                        ok
   one reveal withheld: INCOMPLETE, no ruling, j6 recorded as missing ok
   audit report before the discussion phase: refused                  ok
   second commit from one juror: refused                              ok
   reveal with changed scores: refused                                ok
   reveal with a different salt: refused                              ok
   reveal with a changed rationale hash: refused                      ok
   second reveal from one juror: refused                              ok
   correct reveals from all 6: DECIDED, worker wins                   ok
== 4. Sandbox escrow: payment follows the jury outcome (each deal holds 2 NEAR)
   INCONCLUSIVE case: settle leaves 2 NEAR held                       ok
   INCOMPLETE case: settle leaves 2 NEAR held                         ok
   worker balance did not change                                      ok
   DECIDED worker wins: deal status is PaidWorker                     ok
   worker received exactly 2 NEAR                                     ok
   second settle on the paid deal: refused                            ok
   worker balance is still plus 2 NEAR after the retry                ok
   DECIDED requester wins: deal status is RefundedRequester           ok
   buyer received exactly 2 NEAR                                      ok
   transfer to a missing account fails: deal returns to Held          ok
   retry after the account exists: PaidWorker                         ok
   that worker received exactly 2 NEAR, one time                      ok
   escrow balance: 3 deals paid out, 2 deals still held               ok
== 5. Replay 3 recorded panels (fixture type: recorded puzzle panel)
   pushJ-A00-s0   Pavel   DECIDED       Pavel           Pavel            24.9%  4/6
   pushJ-A02-s0   Nils    DECIDED       Nils            Nils             41.0%  5/6
   pushJ-A03-s0   Cora    DECIDED       Cora            Cora             29.6%  4/6
ALL EXPECTED RESULTS MATCH
```

The withheld-reveal test waits for an 8-second deadline.

### Step 3. Replay all 47 recorded panels

```bash
make demo
```

This takes about 2 minutes after the build. Expected result:

```
== 6. Result of the recorded replay
   contract result equals the offline calculation:    47 of 47
   contract outcome: 42 decided and correct, 0 decided and wrong, 5 inconclusive
   top option of the first reads is correct:          47 of 47
   top option of the audit reads:                     42 correct, 4 wrong, 1 tie
   same jurors at both stages, first reads correct:   44 of 47
   same jurors at both stages, audit reads correct:   42 of 47
ALL EXPECTED RESULTS MATCH
```

The program exits with an error if a result differs from the expected result.

### Step 4. Fresh run with new model calls (optional, needs a key)

Copy `.env.example` to `.env` and put your Adaption key in it. Then run:

```bash
make fresh-one   # one dispute, about 2 minutes
make fresh       # ten disputes, about 25 minutes
```

The default panel is six models on Adaption, the same six as the recorded panels. Pick other jurors with `--jurors`, for example `near:z-ai/glm-5.3` for a model on NEAR AI Cloud. A panel of six models on NEAR AI Cloud ran on 11 November. That run needs `NEAR_AI_API_KEY` in `.env`.

In each case the harness opens the case, asks each juror for a first read, and sends the commits. After five of six commits it tries to release the first reads, and it stops with an error if that works. After the sixth commit each juror sees the first reads of its peers, writes one message, and gives a second read. The harness posts the second reads, then the reveals, then it settles a 2 NEAR sandbox deal.

Result of the run on 10 November 2026 (`runs/fresh-2026-11-10/`):

```
   case                     expected        outcome       ruling          first reads     after talk       moved  audit  escrow
   synthetic:dispute:01:v1  requester wins  DECIDED       requester wins  requester wins  requester wins    0.0%  6/6  RefundedRequester
   synthetic:dispute:02:v1  worker wins     DECIDED       worker wins     worker wins     worker wins       2.2%  6/6  PaidWorker
   synthetic:dispute:03:v1  requester wins  DECIDED       requester wins  requester wins  requester wins    5.8%  6/6  RefundedRequester
   synthetic:dispute:04:v1  redo            DECIDED       redo            redo            redo             25.0%  6/6  Held
   synthetic:dispute:05:v1  worker wins     DECIDED       worker wins     worker wins     worker wins       1.7%  6/6  PaidWorker
   synthetic:dispute:06:v1  requester wins  DECIDED       requester wins  requester wins  requester wins   17.5%  6/6  RefundedRequester
   synthetic:dispute:07:v1  worker wins     DECIDED       worker wins     worker wins     worker wins      20.8%  6/6  PaidWorker
   synthetic:dispute:08:v1  redo            DECIDED       redo            redo            redo             15.8%  6/6  Held
   synthetic:dispute:09:v1  redo            INCONCLUSIVE  no ruling       requester wins  redo             18.3%  6/6  Held
   synthetic:dispute:10:v1  worker wins     DECIDED       requester wins  requester wins  worker wins      15.0%  6/6  RefundedRequester
```

The contract decided 8 cases with the expected ruling. Case 09 has a hidden submission, and the panel reached no majority, which is the outcome that the fixture expects. Case 10 is late delivery of correct work: the first reads gave "requester wins" 56%, and the fixture expects "worker wins". After the discussion the top option equals the expected ruling in all 10 cases. This is one run of ten cases that one person wrote. It is not evidence for or against locked first reads. The model answers change from run to run.

Result of the run on 11 November 2026 with six models on NEAR AI Cloud (`runs/fresh-2026-11-11-near/`). The panel was Qwen3.8-27B, Qwen3.6-35B-A3B, DeepSeek-V4.1-Flash, GLM-5.3, GLM-5.3-flash, and Kimi-K2.6:

```
   case                     expected        outcome       ruling          first reads     after talk       moved  audit  escrow
   synthetic:dispute:01:v1  requester wins  DECIDED       requester wins  requester wins  requester wins    0.0%  6/6  RefundedRequester
   synthetic:dispute:02:v1  worker wins     DECIDED       worker wins     worker wins     worker wins       2.3%  6/6  PaidWorker
   synthetic:dispute:03:v1  requester wins  DECIDED       requester wins  requester wins  requester wins    4.2%  6/6  RefundedRequester
   synthetic:dispute:04:v1  redo            DECIDED       redo            redo            redo              5.0%  6/6  Held
   synthetic:dispute:05:v1  worker wins     DECIDED       worker wins     worker wins     worker wins       2.7%  6/6  PaidWorker
   synthetic:dispute:06:v1  requester wins  DECIDED       requester wins  requester wins  requester wins    9.2%  6/6  RefundedRequester
   synthetic:dispute:07:v1  worker wins     DECIDED       worker wins     worker wins     worker wins       8.0%  6/6  PaidWorker
   synthetic:dispute:08:v1  redo            DECIDED       redo            redo            redo              2.7%  6/6  Held
   synthetic:dispute:09:v1  redo            DECIDED       worker wins     worker wins     worker wins      24.2%  6/6  PaidWorker
   synthetic:dispute:10:v1  worker wins     INCONCLUSIVE  no ruling       worker wins     requester wins    4.2%  6/6  Held
```

The contract decided 8 cases with the expected ruling, and its result equals the offline calculation in all 10. In case 09 the submission is hidden from the jurors, and this panel paid the worker. The fixture expects no payment on that record. In case 10 the panel reached no majority. The two panels differ on cases 09 and 10. Two runs of ten cases do not show which panel is better.

### Step 5. Check the EVM adapter with the recorded testnet signatures

You need Node.js for this step. The run on 11 November used Node.js 22. `npm ci` puts Foundry and the Solidity libraries into `evm/node_modules`. You do not need a key or a NEAR account.

```bash
make evaluator-check
```

Expected result, in the last lines of each part:

```
test result: ok. 16 passed; 0 failed
test result: ok. 5 passed; 0 failed
5737b69b4a81013128480187cd99686dc83320a88040ba11cdd3ea0389392f41
test vector ok: this script, the contract, and the adapter agree
[PASS] test_recorded_signature_for_the_worker_pays_the_provider()
[PASS] test_recorded_signature_for_the_requester_refunds_the_client()
[PASS] test_each_recorded_signature_recovers_to_the_derived_signer_address()
[PASS] test_the_near_contract_and_the_adapter_compute_the_same_digest()
[PASS] test_recorded_signature_with_a_changed_verdict_is_refused()
[PASS] test_recorded_signature_for_another_job_is_refused()
[PASS] test_adapter_with_another_signer_refuses_the_recorded_signature()
[PASS] test_second_call_with_a_recorded_signature_is_refused()
Ran 2 test suites in 15.12ms (16.91ms CPU time): 19 tests passed, 0 failed, 0 skipped (19 total tests)
```

The first two lines are the unit tests of `evaluator/` and of the signature conversion. Each Foundry line also shows a gas amount, and the times change from run to run. The 11 other Foundry tests sign with a local test key.

### Step 6. Live run on NEAR testnet (optional, uses the network)

```bash
make evaluator-live
```

The testnet faucet funds the new accounts, so you need no account and no key. The run takes about 3 minutes. It writes new signatures to `fixtures/evaluator/testnet-rulings.json`. Each run makes new contract accounts, so the signer address changes. Expected result:

```
   jury account: full access key deleted, 0 keys left   ok
   evaluator account: full access key deleted, 0 keys left   ok
   synthetic-dispute-02-v1: jury case 0 on testnet is DECIDED, worker wins, as in the recorded run   ok
   synthetic-dispute-02-v1: the signer network signed the ruling (18.6 Tgas burnt)   ok
   synthetic-dispute-01-v1: jury case 1 on testnet is DECIDED, requester wins, as in the recorded run   ok
   synthetic-dispute-01-v1: the signer network signed the ruling (18.6 Tgas burnt)   ok
LIVE RUN COMPLETE
```

Run `make evaluator-check` again after a live run. It then checks the new signatures.

### Step 7. Open the viewer

```bash
make viewer
```

Expected result: a line that starts with `wrote viewer/index.html` and ends with the number of commitments verified. Then open `viewer/index.html` in a browser. The page holds its data and its fonts, so it needs no server and no network.

The page shows one case at a time: the ruling, the sandbox payment, a triangle with each juror before and after the discussion, the pooled reads, the evidence, and the fixture's expected ruling in a separate record. A list at the side selects the case, and a menu selects the run. The build script computes each commitment hash again from the revealed values and stops with an error if one differs.


## What each mode shows

| Mode | What it shows | Status |
|---|---|---|
| Recorded replay | The contract accepts valid inputs, refuses invalid inputs, and computes the expected outcome. The model calls and the discussion happened before the replay. The replay does not show when they happened. | Built |
| Fresh run | New model calls go through the harness. The harness waits for the commit transactions to finish before it shares a juror message with a peer. | Built. One run of ten disputes is in `runs/fresh-2026-11-10/`. |

The contract records each first-read commitment before it opens the next phase. Model execution and communication outside the harness are outside this proof.

The panel is operator-run. One operator holds all six keys in the demo. Six accounts show six identities. They do not show six independent parties.

## The fixtures

| Fixture type | What it supports | In this repo |
|---|---|---|
| Recorded puzzle panel | Contract and decision-rule tests on real model outputs with a known answer | 47 in `cases/pushJ/` |
| Synthetic panel with a stated expected outcome | Attack tests and escrow tests | 4, in `demo/src/main.rs` |
| Synthetic dispute with a stated expected outcome | A fresh run with real jurors | 10 in `cases/synthetic-disputes/`. One fresh run has read them. |
| Partial public case | A picture of the market's dispute workflow | 158 in `cases/market-legacy/index.json` |
| Complete authorized market case | Comparison with the market's ruling | None |

The 47 panels come from the Apart experiment (`runs/pushJ.jsonl`, condition C2, free discussion, each juror saw half of the clues). Six open-weight models judged fictional whodunit cases. `scripts/export_cases.py` wrote the case files. It scaled each read to sum to 10,000 and changed nothing else. The experiment has 60 such panels. The 47 are the panels where all six jurors gave a usable first read. The rationale hash of each juror is the hash of its logged first-read output.

## Limits

- **The recorded cases are whodunit puzzles.** They are not market disputes.
- **The evidence for locked reads is suggestive.** On the same jurors at both stages the count is 44 against 42 on these panels. On the full experiment it was 57 against 53, p = 0.29. Some audit reads are missing because model outputs were cut off. The last column of the replay shows how many jurors posted one.
- **The contract keeps what the jurors send.** A hostile instruction in the evidence can change all six first reads before the commit. The contract will keep those reads.
- **An audit report is a report from the juror account.** The contract cannot check the model call behind it.
- **The evidence hash shows that a bundle did not change.** It does not show that the bundle is complete or real.
- **One juror can stop a case** by withholding a commit or a reveal.
- **The replay uses fixed salts** so that each run prints the same result. A live juror must use 32 random bytes. `scripts/commit_hash.py` has `new_salt()`.
- **The panel is fixed** when the contract starts.
- **The EVM side of the evaluator runs on a local EVM.** No job on a public EVM chain is settled yet.
- **The live run on testnet is a replay.** It used the recorded reads of two synthetic disputes. It shows the signing path. It is not a new jury run.
- **The adapter does not check the deliverable.** The ERC-8183 reference contract emits the deliverable hash in an event and does not store it. The adapter checks the hash of the job description. A reader compares the deliverable with the evidence off-chain.
- **Only a ruling for the worker or the requester gets a signature.** With a "redo" ruling or no decision, the job expires, and then ERC-8183 lets anyone call `claimRefund` for the client.
- **`evaluator/` signs only the cases that one opener account opened.** In the live run that account is the operator, and the operator also runs the six jurors.
- **The adapter cannot send out tokens.** Use it with an escrow that pays no evaluator fee.

## Not built

- **A settled job on a public EVM chain.** The adapter is not deployed on Arc Testnet or on any other public chain.
- **A complete market case.** It needs an export from a buyer or a worker who allows public use.
- **Split rulings.** The market can split a payment. This contract has no split outcome.

## Relation to the Apart project

The Apart repository holds the experiment and its logs. This repository is new code written from 10 November 2026: the contract, the replay, and the scripts. The case files are data exported from the Apart logs.

## Files

- `contract/src/lib.rs`: the jury contract and 27 unit tests.
- `escrow/src/lib.rs`: the sandbox escrow and 3 unit tests.
- `demo/src/main.rs`: the sandbox replay, the attack tests, and the escrow tests.
- `demo/src/bin/fresh.rs`: the fresh-run harness.
- `demo/src/lib.rs`: sandbox helpers that both programs use.
- `runs/fresh-2026-11-10/`: the records of one fresh run, one file for each dispute.
- `scripts/build_viewer.py` and `viewer/template.html`: build the viewer. `viewer/index.html` is the built page for the two fresh runs.
- `scripts/fetch_market_cases.py`: reads public dispute records from the legacy market.
- `cases/market-legacy/index.json`: 158 partial public cases.
- `cases/synthetic-disputes/disputes.json`: 10 synthetic disputes.
- `scripts/commit_hash.py`: the commitment hash for the harness, with the shared test vector.
- `scripts/export_cases.py`: makes the case files from the Apart logs.
- `cases/pushJ/`: 47 recorded panels.
- `evaluator/src/lib.rs`: the evaluator contract and 16 unit tests.
- `evm/src/LockJuryEvaluator.sol`: the EVM adapter. `evm/test/` has 19 Foundry tests.
- `evm/lib/erc-8183/`: the ERC-8183 reference contract, copied from erc-8183/base-contracts at commit 142e669.
- `demo/src/bin/evaluator_testnet.rs`: the live run on NEAR testnet.
- `fixtures/evaluator/testnet-rulings.json`: the signatures and the values of the live run on 11 November.
- `scripts/ruling_digest.py`: the ruling digest for the harness, with the shared test vector.
- `pitches/`: the two shaped pitches.
- `cycles/`: the record of the second bet.

## License

MIT.

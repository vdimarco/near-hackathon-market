# Lock Before You Talk: a jury on NEAR

Built for NEAR HACKATHON: Build Beyond Boundaries. Tracks: Agent Economy, AI Swarms, AI Security & Defense.

**Status on 10 November 2026:** the contract, its tests, and the recorded replay work. The sandbox escrow, the market case fixtures, the fresh-run harness, and the viewer page are not built. See "Not built".

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

## NEAR tools used

- **A Rust contract** with `near-sdk` 5.30 (`contract/`).
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
49cb452949724da1a78ea133ddb650af1b8cfd4ef3db6b648b565afd0214fd87
test vector ok: this script and the contract agree
```

### Step 2. Run the attack tests and 3 recorded panels

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
== 4. Replay 3 recorded panels (fixture type: recorded puzzle panel)
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
== 5. Result of the recorded replay
   contract result equals the offline calculation:    47 of 47
   contract outcome: 42 decided and correct, 0 decided and wrong, 5 inconclusive
   top option of the first reads is correct:          47 of 47
   top option of the audit reads:                     42 correct, 4 wrong, 1 tie
   same jurors at both stages, first reads correct:   44 of 47
   same jurors at both stages, audit reads correct:   42 of 47
ALL EXPECTED RESULTS MATCH
```

The program exits with an error if a result differs from the expected result.

## What each mode shows

| Mode | What it shows | Status |
|---|---|---|
| Recorded replay | The contract accepts valid inputs, refuses invalid inputs, and computes the expected outcome. The model calls and the discussion happened before the replay. The replay does not show when they happened. | Built |
| Fresh run | New model calls go through the harness. The harness waits for the commit transactions to finish before it shares a juror message with a peer. | Not built |

The contract records each first-read commitment before it opens the next phase. Model execution and communication outside the harness are outside this proof.

The panel is operator-run. One operator holds all six keys in the demo. Six accounts show six identities. They do not show six independent parties.

## The fixtures

| Fixture type | What it supports | In this repo |
|---|---|---|
| Recorded puzzle panel | Contract and decision-rule tests on real model outputs with a known answer | 47 in `cases/pushJ/` |
| Synthetic case with a stated expected outcome | Attack tests | 3, in `demo/src/main.rs` |
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

## Not built

- **A sandbox escrow** that follows the outcome: pay on `Decided`, hold on `Inconclusive` and `Incomplete`, safe to retry.
- **Market case fixtures.** The market shows dispute evidence only to the buyer, the worker, and the resolver. A complete case needs an export from a participant.
- **The fresh-run harness** with new model calls.
- **The one-page viewer.**
- **Split rulings.** The market can split a payment. This contract has no split outcome.

## Relation to the Apart project

The Apart repository holds the experiment and its logs. This repository is new code written from 10 November 2026: the contract, the replay, and the scripts. The case files are data exported from the Apart logs.

## Files

- `contract/src/lib.rs`: the contract and 27 unit tests.
- `demo/src/main.rs`: the sandbox replay and the attack tests.
- `scripts/commit_hash.py`: the commitment hash for the harness, with the shared test vector.
- `scripts/export_cases.py`: makes the case files from the Apart logs.
- `cases/pushJ/`: 47 recorded panels.
- `pitches/`: the shaped pitch.

## License

MIT.

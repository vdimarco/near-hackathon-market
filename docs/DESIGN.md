# Design

This file describes the jury contract, the sandbox escrow, the two run modes, and the fixtures. The [README](../README.md) has the short version.

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

## What each mode shows

| Mode | What it shows | Status |
|---|---|---|
| Recorded replay | The contract accepts valid inputs, refuses invalid inputs, and computes the expected outcome. The model calls and the discussion happened before the replay. The replay does not show when they happened. | Built |
| Fresh run | New model calls go through the harness. The harness waits for the commit transactions to finish before it shares a juror message with a peer. | Built. Two runs of ten disputes are in `runs/`. |

The contract records each first-read commitment before it opens the next phase. Model execution and communication outside the harness are outside this proof.

The panel is operator-run. One operator holds all six keys in the demo. Six accounts show six identities. They do not show six independent parties.

## The fixtures

| Fixture type | What it supports | In this repo |
|---|---|---|
| Recorded puzzle panel | Contract and decision-rule tests on real model outputs with a known answer | 47 in `cases/pushJ/` |
| Synthetic panel with a stated expected outcome | Attack tests and escrow tests | 4, in `demo/src/main.rs` |
| Synthetic dispute with a stated expected outcome | A fresh run with real jurors | 10 in `cases/synthetic-disputes/`. Two fresh runs have read them. |
| Partial public case | A picture of the market's dispute workflow | 158 in `cases/market-legacy/index.json` |
| Complete authorized market case | Comparison with the market's ruling | None |

The 47 panels come from the Apart experiment (`runs/pushJ.jsonl`, condition C2, free discussion, each juror saw half of the clues). Six open-weight models judged fictional whodunit cases. `scripts/export_cases.py` wrote the case files. It scaled each read to sum to 10,000 and changed nothing else. The experiment has 60 such panels. The 47 are the panels where all six jurors gave a usable first read. The rationale hash of each juror is the hash of its logged first-read output.

# Run it, step by step

Every step states the command and the expected result. `make check` runs steps 1, 2, 5, and 7 in one command.

You need Rust (rustup) and Python 3. The file `rust-toolchain.toml` selects Rust 1.97.0 and the `wasm32-unknown-unknown` target. You do not need an API key, a wallet, or a NEAR account.

## Step 1. Run the contract tests

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

## Step 2. Run the attack tests, the escrow tests, and 3 recorded panels

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

## Step 3. Replay all 47 recorded panels

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

## Step 4. Fresh run with new model calls (optional, needs a key)

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

## Step 5. Check the EVM adapter with the recorded testnet signatures

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

## Step 6. Live run on NEAR testnet (optional, uses the network)

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

## Step 7. Open the viewer

```bash
make viewer
```

Expected result: a line that starts with `wrote viewer/index.html` and ends with the number of commitments verified. Then open `viewer/index.html` in a browser. The page holds its data and its fonts, so it needs no server and no network.

The page shows one case at a time: the ruling, the sandbox payment, a triangle with each juror before and after the discussion, the pooled reads, the evidence, and the fixture's expected ruling in a separate record. A list at the side selects the case, and a menu selects the run. The build script computes each commitment hash again from the revealed values and stops with an error if one differs.

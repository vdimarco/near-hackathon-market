# Lock Before You Talk: a jury on NEAR

Built for NEAR HACKATHON: Build Beyond Boundaries. Tracks: Agent Economy, AI Swarms, AI Security & Defense.

## What it does

A panel of AI jurors rules on a dispute. Each juror locks its first read on NEAR before the jurors talk. The contract makes the ruling from the locked reads. Each juror can also post its read after the discussion, so anyone can see which jurors moved.

## The problem

On NEAR Agent Market (market.near.ai) an agent delivers a job and waits. If the requester does not review the work in 24 hours, the market opens a dispute, and one resolver rules. A worker agent cannot check how the ruling was made.

A panel of AI jurors can rule for cents, but discussion makes jurors agree for bad reasons. In my Apart Research experiment ([apart-swarm-alignment](https://github.com/vdimarco/apart-swarm-alignment)), error correlation between model pairs went from 0.42 to 0.77 after free discussion. Six agents became 1.2 to 1.8 effective forecasters. In that experiment the lock was a sentence in the prompt. This contract makes the lock real.

## How it works

1. `open_case(job_id, evidence_hash, options, default_option, commit_secs, reveal_secs)` records a dispute.
2. Each juror calls `commit(case_id, hash)`. The hash is `sha256("case_id:juror:p0,p1,...:salt")`. Probabilities are integers in basis points and sum to 10,000.
3. The contract opens the discussion when the last juror commits, or when someone calls `close_commits` after the deadline. The harness reads `is_discussion_open` before it sends a discussion prompt.
4. Each juror calls `reveal(case_id, probs, salt)`. The contract checks the hash.
5. Each juror can call `final_read(case_id, probs)` with its read after the discussion.
6. `finalize(case_id)` pools the revealed first reads. The ruling is the top option if it has more than 50%. If not, the ruling is `default_option`.
7. `get_case(case_id)` shows the commits, both reads of each juror, how far each juror moved, the ruling, and what the ruling would be from the reads after the discussion.

A juror that commits and does not reveal is left out of the pool after the reveal deadline. The record shows it.

## NEAR tools used

- **A Rust contract** with `near-sdk` 5.30 (`contract/`).
- **Sub-accounts.** Each juror is `j1.jury.<root>` to `j6.jury.<root>`.
- **Function call keys.** Each juror account keeps one key. That key can call `commit`, `reveal`, and `final_read` on the jury contract and nothing else. The demo deletes the full access key of each juror.
- **NEAR sandbox** through `near-workspaces` 0.24 (`demo/`).

## Run it

You need Rust (rustup). The file `rust-toolchain.toml` installs the `wasm32-unknown-unknown` target. You do not need an API key, a wallet, or a NEAR account.

### Step 1. Run the contract tests

```bash
make test
```

Expected result: `test result: ok. 11 passed; 0 failed`.

### Step 2. Replay 3 recorded panels on a local sandbox

```bash
make demo-quick
```

The first build downloads the NEAR sandbox and takes a few minutes. Expected result:

```
== 3. Check the juror key limits
   juror key sends 1 NEAR to another account: blocked
   juror key calls open_case:                 blocked
== 4. Replay 3 recorded panels
   pushJ-A00-s0     Pavel    Pavel            Pavel                24.9%  4/6
   pushJ-A02-s0     Nils     Nils             Nils                 41.0%  5/6
   pushJ-A03-s0     Cora     Cora             Cora                 29.6%  4/6
== 5. Result
   reveals before commits closed, blocked: 3 of 3
   top option of the locked first reads is correct: 3 of 3
```

### Step 3. Replay all 47 recorded panels

```bash
make demo
```

This takes about 2 minutes after the build. Expected result:

```
== 5. Result
   reveals before commits closed, blocked: 47 of 47
   top option of the locked first reads is correct: 47 of 47
   top option of the reads after talk:              42 correct, 4 wrong, 1 tie or no read
   same jurors at both stages, first reads correct: 44 of 47
   same jurors at both stages, after talk correct:  42 of 47
   contract ruling (one option above 50%) correct:  42 of 47
```

For each panel the demo does these things on the sandbox:

- The requester opens a case.
- The first juror commits and then tries to reveal. The contract refuses, because commits are open.
- The other jurors commit. The contract opens the discussion.
- All jurors reveal. Jurors with a recorded read after the discussion post it.
- The requester calls `finalize`. The demo reads the result with `get_case`.

The column `moved` is the mean distance between the first read and the read after the discussion, for the jurors that posted both.

## Where the numbers come from

The 47 case files in `cases/pushJ/` are recorded model outputs from the Apart experiment (`runs/pushJ.jsonl`, condition C2, free discussion). Six open-weight models solved fictional whodunit cases with a known answer. Each juror saw half of the clues. `scripts/export_cases.py` wrote the case files. It changed nothing except that it scaled each read to sum to 10,000. The experiment has 60 such panels. The 47 are the panels where all six jurors gave a usable first read.

## Limits

- **The cases are whodunit puzzles.** They are not Agent Market disputes. A replay of real disputed jobs from market.near.ai is the next step.
- **The evidence is suggestive.** On the same jurors at both stages the result is 44 against 42 on these panels. On the full experiment it was 57 against 53, p = 0.29. Some reads after the discussion are missing because model outputs were cut off. The last column of the demo shows how many jurors posted one.
- **The proof has a limit.** The contract proves that each commit came before the discussion that the harness ran. It cannot prove that jurors did not talk on a different channel.
- **The replay uses fixed salts** so that each run prints the same result. A live juror must use 32 random bytes.
- **The ruling moves no money.** An escrow that follows the ruling is not built.
- **The panel is fixed** when the contract starts.

## Files

- `contract/src/lib.rs`: the contract and 11 unit tests.
- `demo/src/main.rs`: the sandbox replay.
- `cases/pushJ/`: 47 recorded panels.
- `scripts/export_cases.py`: makes the case files from the Apart logs.

## License

MIT.

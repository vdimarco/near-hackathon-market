# Cycle 2: the jury as an evaluator for escrows on other chains

**Pitch:** [pitches/evaluator-on-other-chains.md](../pitches/evaluator-on-other-chains.md)

**Bet:** 10 November 2026, 22:50 Montreal time. The owner confirmed the build.

**Appetite:** One session. It started when the bet was made.

**Start condition:** The Wednesday work of the first pitch is done. The fresh-run harness ran on 10 November.

**Shipped means:** `make evaluator-check` passes from a fresh checkout. A real signature from `v1.signer-prod.testnet` for a closed jury case makes the adapter pay the provider of an ERC-8183 job. The adapter refuses a changed verdict, another job id, another signer, and a second call. The branch is merged to `main`.

**Circuit breaker:** If that is not true at the end of the session, the bet stops. The work stays on the branch `evaluator-on-other-chains`, and the README does not mention it.

## Scopes

The work showed these scopes. A `~` marks a scope that the session can drop.

| Scope | State on 11 November 2026, 04:40 UTC |
|---|---|
| Signer call | Done. One call from an account, then two calls from the evaluator contract on testnet. |
| Ruling digest | Done. The script, the NEAR contract, and the adapter give the same digest for one test vector. |
| Adapter | Done. 11 Foundry tests with a local key, written before the code. |
| Evaluator contract | Done. 16 unit tests, written before the code. |
| Testnet replay | Done. `make evaluator-live` ran one time to the end. |
| Recorded check | Done. `make evaluator-check` passes. 8 Foundry tests use the two testnet signatures. |
| ~ One transaction on Arc Testnet | Dropped. Not started. |
| Merge to `main` | Open. The owner decides. |

## What the work showed

- **No owner account was necessary.** The testnet faucet gives each new account 10 NEAR. The pitch asked the owner for an account.
- **The signer contract has a `derived_public_key` view.** It gives the key for a contract account and a path. The fixture takes the signer address from this view, so the recorded check does not take the address from the signatures.
- **The compressed point of the answer does not give `v`.** One answer had the prefix `03` and the recovery id 0. The conversion uses the recovery id.
- **The public RPC server of testnet refused requests in the first run.** The answer was "this client has exceeded the rate limit". The program now uses the FastNEAR server, waits 0.4 seconds before each request, and sends a refused request again.
- **Attached gas costs NEAR before a call runs.** A juror account with 0.3 NEAR could not attach 300 Tgas. Juror calls now attach 20 Tgas.
- **The reference contract at commit 142e669 is larger than the contract in the EIP text.** `createJob` takes six arguments, and an admin must allow the payment token. The deliverable hash is still only in an event.
- **Two rulings went through, not one.** Dispute 02 gives "worker wins", and dispute 01 gives "requester wins". The recorded check thus covers `complete` and `reject`.

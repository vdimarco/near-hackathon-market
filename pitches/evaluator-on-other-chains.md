# The jury as an evaluator for escrows on other chains

**Draft 1, 10 November 2026.** A second bet for the repository of "Lock Before You Talk". It is not approved.

**Appetite:** One session, Wednesday 11 November. The first pitch gave that session to the fresh-run harness, and the harness ran on Tuesday.

## Problem

An ERC-8183 job names one evaluator address. After the provider submits, only that address can pay the provider or refund the client. The standard states that a hostile evaluator can do either.

The evaluator must act on the chain of the escrow. Ratify, an evaluator on GenLayer, settles an escrow that is on GenLayer Chain. The check of 10 November found ERC-8183 escrows on Arc and Base, and no evaluator that signs through NEAR Chain Signatures.

The hackathon asks for work that crosses chains. Our jury rules on NEAR, and a ruling can move money only in our sandbox escrow.

## Solution

A NEAR contract signs each ruling with a key that only its account can use. A small contract on the EVM chain trusts that key and settles the job.

1. **Name the evaluator.** The client creates the ERC-8183 job with our adapter contract as its evaluator.
2. **Open the case.** The case key holds the chain id, the escrow contract, the job id, the adapter, and the hash of the job description.
3. **Rule.** The jury runs as it does today. No method of the jury contract changes.
4. **Sign.** Anyone calls `sign_ruling` on a new contract, `evaluator/`. The call gives the job fields, and the contract refuses a case whose key differs from them. It reads the case with `get_case`, as the escrow does. For a `Decided` case with the ruling "worker wins" or "requester wins" it builds a 32-byte digest and asks `v1.signer-prod.testnet` to sign it.
5. **Settle.** Anyone sends the signature to the adapter. The adapter builds the same digest from the job on its own chain and checks the signer. Then it calls `complete` or `reject`.

The digest binds the chain id, the adapter, the escrow contract, the job id, the description hash, the verdict, and a reason. The reason is a hash of the jury account, the case id, the evidence hash, and the policy hash.

**Two modes**, as in the first pitch. The recorded check needs no key. It replays one real signature from the testnet signer against the adapter in a Foundry test. The live run asks the signer again.

## Rabbit holes

- **The live run needs the jury on NEAR testnet.** The sandbox has no signer. Deploy the jury with its six juror accounts, and `evaluator/`, under one funded account. Replay the recorded reads of synthetic dispute 02 from `runs/fresh-2026-11-10/`. The replay shows the signing path and is not a new jury run.
- **The `sign` call.** Another project, bazaar-of-liars, calls the testnet signer from a contract with 1 yoctoNEAR and 100 Tgas. It reports that a transaction can succeed with no signature. Make one call in the first hour. `evaluator/` records a signature only in the callback, and `sign_ruling` can run again.
- **The signer address and the signature format.** Sign one fixed digest and recover the address from the signature. The adapter gets that address when it is deployed. A script converts the answer of the signer to the Ethereum form. The Rust code and the Solidity code share one test vector for the digest.
- **Control of the signing account.** A full access key of `evaluator/` or of the jury account can replace the code. The live run deletes both keys after the deploy.
- **Who opens a case.** Anyone can call `open_case`. `evaluator/` signs only the cases that its configured opener account opened.
- **The deliverable hash.** The reference contract in the EIP emits it in an event and does not store it. The adapter checks the description hash. A reader compares the deliverable hash with the evidence off-chain. The README says so.
- **Other results.** Only a ruling for the worker or the requester gets a signature. With no signature the job expires, and then anyone can call `claimRefund` for the client. ERC-8183 has no redo.
- **No public EVM chain is necessary.** The Foundry test deploys the reference contract at a pinned commit. It sets the chain id and the adapter address to the values in the signed digest. One transaction on Arc Testnet, which hosts the reference contract, is the first thing to cut.

## No-gos

- No change to the jury contract, the escrow, the recorded replay, or the fresh-run harness.
- No mainnet and no NEAR Intents. No token moves between chains.
- No EVM transaction from the signer. It signs one digest for each case.
- No ERC-8183 hook and no ERC-8004 feedback.
- No Solana and no second EVM chain.
- No model call.
- No claim that the panel is independent. One operator runs the six jurors.

## Session

| Session | Work | Exit condition |
|---|---|---|
| Wed 11 Nov | First hour: one `sign` call on testnet from a test contract. Then `evaluator/`, the adapter, the digest test vector, the testnet replay, and the Foundry check. | `make evaluator-check` passes from a fresh checkout. A real signature from the testnet signer for a closed jury case makes the adapter pay the provider of an ERC-8183 job. The adapter refuses a changed verdict, another job id, another signer, and a second call. |

**Circuit breaker.** If the exit condition is not met at the end of the session, the bet stops. The work stays on a branch, and the README does not mention it. Thursday and Friday keep their plans.

**Before the session.** The build needs a NEAR testnet account with about 10 NEAR. It adds Foundry to the repository, and only `make evaluator-check` uses it.

**Checked on 10 November 2026:** [ERC-8183](https://eips.ethereum.org/EIPS/eip-8183), [Ratify](https://github.com/JspIIV/ratify), [NEAR docs on signing](https://docs.near.org/chain-abstraction/chain-signatures/implementation), [bazaar-of-liars pull request 7](https://github.com/lopeselio/bazaar-of-liars/pull/7), [Arc contract addresses](https://docs.arc.io/arc/references/contract-addresses).

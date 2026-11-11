# Rulings for escrows on other chains

This file describes the evaluator contract and the EVM adapter. Steps 5 and 6 of [RUN.md](RUN.md) run them.

Only the NEAR side of this part is the hackathon work that the judges assess: the `evaluator/` contract and its use of Chain Signatures. The Solidity adapter exists to show that an EVM contract can check the signature.

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

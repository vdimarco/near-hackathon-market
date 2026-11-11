# Limits and what is not built

## Limits

- **The recorded cases are whodunit puzzles.** They are not market disputes.
- **The evidence for locked reads is suggestive.** On the same jurors at both stages the count is 44 against 42 on these panels. On the full experiment it was 57 against 53, p = 0.29. Some audit reads are missing because model outputs were cut off. The last column of the replay shows how many jurors posted one.
- **The contract keeps what the jurors send.** A hostile instruction in the evidence can change all six first reads before the commit. The contract will keep those reads.
- **An audit report is a report from the juror account.** The contract cannot check the model call behind it.
- **The evidence hash shows that a bundle did not change.** It does not show that the bundle is complete or real.
- **One juror can stop a case** by withholding a commit or a reveal.
- **The replay uses fixed salts** so that each run prints the same result. A live juror must use 32 random bytes. `scripts/commit_hash.py` has `new_salt()`.
- **The panel is fixed** when the contract starts.
- **Anyone can open a case, and the jury contract pays for its storage.** A real service needs a deposit from the opener.
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

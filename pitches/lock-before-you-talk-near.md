# Lock Before You Talk: a jury record for agent payment disputes

**Revision 2, 10 November 2026.** Changed after an outside review. The changes are listed at the end.

**Appetite:** Four sessions, Tuesday to Friday. Saturday is spare time before the deadline at 18:59 Montreal time (23:59 UTC).

**Tracks:** Agent Economy, AI Swarms, AI Security & Defense.

## Problem

Agents hire agents on NEAR Agent Market. When a buyer and a worker disagree, an automated resolver rules, and an operator takes the uncertain cases. The market can rule four ways: the agent wins, the buyer wins, a split, or a redo.

A panel of AI jurors is a natural reviewer for such disputes. A panel has one weak point. After jurors talk, nobody can tell which opinions were formed alone and which were copied. In my exploratory Apart study, measured error correlation rose from 0.42 to 0.77 in the free-discussion arm. The estimate used five model pairs, and its confidence interval includes no change. This result is the reason to keep first reads.

No record exists today of what each juror thought before the discussion.

## Solution

Six jurors assess the same evidence separately and commit their first scores on NEAR. The contract checks all six reveals and applies a fixed decision rule. Each juror also reports its scores after the discussion.

1. **Open.** `open_case` fixes the case key, the evidence hash, the policy hash, and the options.
2. **Commit.** Each juror sub-account commits a hash that binds the contract, the case, the evidence, the policy, the juror, the scores, a rationale hash, and a salt. Each juror signs with a function call key for `commit`, `reveal`, and `final_read`.
3. **Discuss.** The sixth commit opens the discussion. The harness shares juror messages only after that.
4. **Audit.** Each juror posts its scores after the discussion, before any first read is revealed.
5. **Reveal.** Each juror reveals. The sixth valid reveal closes the case.
6. **Outcome.** More than 50% for one option gives `Decided`. A full panel with no majority gives `Inconclusive`. A missing commit or reveal gives `Incomplete`.
7. **Pay (sandbox only).** A separate escrow contract pays on `Decided` and holds the funds on the two other outcomes.

One page shows a case: the source and the mode, the outcome, both pools, each juror's two reads, and the payment status.

**Two modes.** The recorded replay runs without model keys and tests the contract. A fresh run sends new model calls through the harness, which waits for the commits before it shares messages.

## Rabbit holes

- **Market case data.** The market shows dispute evidence only to the buyer, the worker, and the resolver. Spend one hour to get one complete case from a participant who allows public use. If that fails, write ten synthetic disputes with a stated expected outcome. Label each fixture by type.
- **A hidden submission is "not available to this review".** It is not missing work.
- **The market's ruling stays out of the evidence bundle.** It goes in a separate comparison record.
- **Case identity.** The case key holds the market, the job, the assignment, the dispute, and the submission version.
- **Withheld reveal.** The contract returns `Incomplete`. One juror can stop a case. The README says so.
- **Split rulings.** Not supported. Split cases stay out of the agreement count.
- **Escrow callback.** The jury outcome and the payment are separate records. A failed transfer can be retried, and a retry cannot pay twice.
- **Harness.** The Apart harness has two repaired bugs and no run since the repair. Test it with one fresh panel before the fresh-run mode depends on it.
- **Model access.** Test NEAR AI Cloud for one hour. If it fails, the fresh run stays on Adaption.

## No-gos

- No rulings on live market disputes, and no connection to the market's payment system.
- No backlog review and no paid listing on the market in this cycle.
- No staking, no slashing, no appeals, no juror selection.
- No split outcome.
- No mainnet deployment, no NEAR Intents, no TEE attestation.
- No discussion text on-chain.
- No second page in the viewer.

## Sessions

| Session | Work | Exit condition |
|---|---|---|
| Tue 10 Nov | Contract with strict phases and outcomes. Recorded replay. One-hour check for an authorized market case. | Done: `make demo` passes on 47 recorded panels and 3 synthetic attack cases. Open: the market case check, the harness test. |
| Wed 11 Nov | Sandbox escrow. Synthetic dispute fixtures. Fresh-run harness. | Valid inputs settle. Missing or changed reveals stop payment. A retry cannot pay twice. |
| Thu 12 Nov | The one-page viewer. Clean install test. | `make demo` passes from a fresh checkout on a second machine. |
| Fri 13 Nov | README from the tested build. Video. X post. Submission form. | All links and steps work. |

## Changes in revision 2

- The problem no longer says that work goes to dispute after 24 hours. That rule came from the legacy market guide.
- The main problem is now the missing record of first opinions.
- A missing reveal gives `Incomplete`. Revision 1 dropped the juror, which let one juror change the result.
- "No majority" gives `Inconclusive`. Revision 1 mapped it to "redo".
- The commitment binds more fields and uses Borsh.
- Audit reports come before reveals.
- The data plan is one authorized case or synthetic fixtures. Revision 1 planned to pull 20 disputed jobs with a new agent key.
- The Apart numbers carry their uncertainty. "Effective jurors" is removed.
- Backlog review and the paid listing are removed from this cycle.

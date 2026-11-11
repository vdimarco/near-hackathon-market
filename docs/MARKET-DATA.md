# What the public market data shows

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

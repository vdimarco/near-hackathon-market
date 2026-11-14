"""Check the live testnet run of the jury from the chain alone.

Read-only. Python 3 standard library only: no wallet, no key, no Rust, no install.

    python3 scripts/verify_testnet.py

It reads the jury contract, its six juror accounts and the evaluator from NEAR
testnet. It reads every juror transaction again from a NEAR archival node and
checks the lock: every reveal matches its commit, every commit of a case lands
before any audit read of that case, and the ruling comes from the revealed first
reads. Each line starts with "ok" or "FAIL". It exits 1 if a check fails.
"""
import base64
import json
import os
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from commit_hash import commitment_hash  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RPC = "https://rpc.testnet.fastnear.com"
ARCHIVAL = "https://archival-rpc.testnet.fastnear.com"
EXPLORER = "https://testnet.nearblocks.io"
JUROR_METHODS = {"commit", "reveal", "final_read"}

fails = 0
passes = 0


def ok(name, cond, extra=""):
    global fails, passes
    print(("ok   " if cond else "FAIL ") + name + (" · " + extra if extra else ""))
    if cond:
        passes += 1
    else:
        fails += 1


def rpc(method, params, url=RPC):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for attempt in range(3):
        try:
            req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=30) as r:
                out = json.load(r)
            if "error" in out:
                raise RuntimeError(json.dumps(out["error"])[:200])
            return out["result"]
        except Exception:
            if attempt == 2:
                raise
            time.sleep(1.5 * (attempt + 1))


def view(account, method, args=None):
    res = rpc("query", {"request_type": "call_function", "finality": "final", "account_id": account,
                        "method_name": method, "args_base64": base64.b64encode(json.dumps(args or {}).encode()).decode()})
    return json.loads(bytes(res["result"]).decode())


def keys(account):
    return rpc("query", {"request_type": "view_access_key_list", "finality": "final", "account_id": account})["keys"]


def main():
    fx = json.load(open(os.path.join(ROOT, "fixtures/evaluator/testnet-rulings.json")))
    txs = json.load(open(os.path.join(ROOT, "fixtures/evaluator/testnet-jury-txs.json")))["txs"]
    jury, evaluator = fx["jury"], fx["evaluator"]
    print(f"Jury on NEAR testnet: {EXPLORER}/address/{jury}\nEvaluator: {EXPLORER}/address/{evaluator}\n")

    # 1. Keys: nobody holds a full-access key, and each juror key is fenced to three methods
    for name, acct in (("jury", jury), ("evaluator", evaluator)):
        ks = keys(acct)
        ok(f"the {name} contract account has no full-access key", all(k["access_key"]["permission"] != "FullAccess" for k in ks), f"{len(ks)} key(s)")
    jurors = view(jury, "get_jurors")
    ok("the jury has six juror accounts, all sub-accounts of the jury", len(jurors) == 6 and all(j.endswith("." + jury) for j in jurors))
    fenced = 0
    for j in jurors:
        ks = keys(j)
        perms = [k["access_key"]["permission"] for k in ks]
        if perms and all(isinstance(p, dict) and p["FunctionCall"]["receiver_id"] == jury
                         and set(p["FunctionCall"]["method_names"]) <= JUROR_METHODS for p in perms):
            fenced += 1
    ok("each juror holds only a function-call key for commit, reveal and final_read on the jury", fenced == 6, f"{fenced} of 6")

    # 2. Read every juror transaction again from an archival node
    block_cache = {}

    def height(block_hash):
        if block_hash not in block_cache:
            block_cache[block_hash] = rpc("block", {"block_id": block_hash}, ARCHIVAL)["header"]["height"]
        return block_cache[block_hash]

    calls = []
    for t in txs:
        if t["method"] not in JUROR_METHODS:
            continue
        r = rpc("tx", {"tx_hash": t["tx"], "sender_account_id": t["signer"], "wait_until": "NONE"}, ARCHIVAL)
        tx = r["transaction"]
        fc = tx["actions"][0]["FunctionCall"]
        good = "SuccessValue" in r["status"] and tx["receiver_id"] == jury and tx["signer_id"] == t["signer"]
        # the contract runs the call in the first receipt; its block is when the call took effect
        calls.append({"method": fc["method_name"], "juror": tx["signer_id"], "args": json.loads(base64.b64decode(fc["args"])),
                      "block": height(r["receipts_outcome"][0]["block_hash"]), "ok": good, "tx": t["tx"]})
    ok("every juror transaction succeeded on chain and went to the jury", all(c["ok"] for c in calls), f"{len(calls)} transactions")

    n_cases = view(jury, "case_count")
    ok("the jury contract holds two cases", n_cases == 2, str(n_cases))
    for case_id in range(n_cases):
        case = view(jury, "get_case", {"case_id": case_id})
        mine = [c for c in calls if c["args"].get("case_id") == case_id]
        commits = {c["juror"]: c for c in mine if c["method"] == "commit"}
        reveals = {c["juror"]: c for c in mine if c["method"] == "reveal"}
        audits = [c for c in mine if c["method"] == "final_read"]
        ok(f"case {case_id}: 6 commits, 6 audit reads and 6 reveals on chain", len(commits) == 6 and len(audits) == 6 and len(reveals) == 6)

        last_commit = max(c["block"] for c in commits.values())
        first_audit = min(c["block"] for c in audits)
        first_reveal = min(c["block"] for c in reveals.values())
        ok(f"case {case_id}: the sixth commit lands before the first audit read and the first reveal",
           last_commit < first_audit and last_commit < first_reveal,
           f"last commit block {last_commit}, first audit {first_audit}, first reveal {first_reveal}")

        matched = 0
        for j in case["jurors"]:
            c, r = commits.get(j["juror"]), reveals.get(j["juror"])
            if not c or not r:
                continue
            a = r["args"]
            h = commitment_hash(jury, case_id, case["evidence_hash"], case["policy_hash"], j["juror"], a["probs"], a["rationale_hash"], a["salt"])
            if h == c["args"]["hash"] and a["probs"] == j["first"] and a["rationale_hash"] == j["rationale_hash"]:
                matched += 1
        ok(f"case {case_id}: each reveal hashes to its juror's commit and equals the stored first read", matched == 6, f"{matched} of 6")

        sums = [sum(j["first"][i] for j in case["jurors"]) for i in range(len(case["options"]))]
        total = sum(sums)
        top = max(range(len(sums)), key=lambda i: sums[i])
        majority = top if sums[top] * 2 > total else None
        ok(f"case {case_id}: the ruling is the strict majority of the first reads",
           sums == case["first_sums"] and case["outcome"] == "Decided" and case["ruling"] == majority,
           f"{case['options'][top]} {sums[top] / total:.1%} of first reads, {case['outcome']}")

    # 3. Chain Signatures: the evaluator's stored signatures are the ones in the fixture
    same = 0
    for f in fx["rulings"]:
        got = view(evaluator, "get_ruling", {"case_id": f["near"]["case_id"]})
        if got and got["status"] == "Signed" and "0x" + got["digest"] == f["digest"] and got["big_r"] == f["near"]["big_r"] and got["s"] == f["near"]["s"]:
            same += 1
    ok("the evaluator holds a Chain Signatures signature for each ruling, equal to the fixture the EVM test replays", same == len(fx["rulings"]), f"{same} of {len(fx['rulings'])}")

    print(f"\nall {passes} checks passed" if not fails else f"\n{fails} of {passes + fails} checks failed")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()

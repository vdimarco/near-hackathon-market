"""Build the one-page viewer from a fresh-run directory.

Usage: python3 scripts/build_viewer.py [runs/<dir>]

The script reads the run records, checks each commitment hash again with
scripts/commit_hash.py, and writes viewer/index.html. The page holds its
data and needs no server and no network. The script exits with an error if
a commitment does not verify.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from commit_hash import commitment_hash  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


def load(run_dir):
    run = json.loads((run_dir / "run.json").read_text())
    cases = []
    for entry in run["cases"]:
        rec = json.loads((run_dir / entry["file"]).read_text())
        chain = rec["chain"]
        by_account = {j["juror"]: j for j in chain["jurors"]}
        jurors = []
        for j in rec["jurors"]:
            on_chain = by_account[j["account"]]
            first = j.get("first") or {}
            verified = None
            if on_chain.get("first") is not None and first.get("salt"):
                expected = commitment_hash(
                    rec["contract"], int(rec["chain_case_id"]), rec["evidence_hash"], rec["policy_hash"],
                    j["account"], on_chain["first"], on_chain["rationale_hash"], first["salt"],
                )
                verified = expected == first.get("commit_hash")
                if not verified:
                    sys.exit(f"commitment does not verify: {rec['case_key']} {j['account']}")
            jurors.append({
                "account": j["account"],
                "model": j["provider_model"],
                "committed": on_chain["committed"],
                "commit_hash": first.get("commit_hash"),
                "verified": verified,
                "first": on_chain.get("first"),
                "audit": on_chain.get("audit"),
                "audit_posted": on_chain.get("audit_posted"),
                "moved_bps": on_chain.get("moved_bps"),
                "first_text": (first.get("text") or "").strip(),
                "message": (j.get("discussion_message") or "").strip(),
                "audit_text": ((j.get("after_discussion") or {}).get("text") or "").strip(),
            })
        cases.append({
            "case_key": rec["case_key"],
            "pattern": rec.get("pattern"),
            "fixture_type": rec["fixture_type"],
            "mode": rec["mode"],
            "contract": rec["contract"],
            "chain_case_id": rec["chain_case_id"],
            "evidence": rec["evidence"],
            "evidence_hash": rec["evidence_hash"],
            "policy_hash": rec["policy_hash"],
            "options": rec["options"],
            "outcome": chain["outcome"],
            "ruling": chain["ruling"],
            "first_sums": chain["first_sums"],
            "audit_sums": chain["audit_sums"],
            "audit_count": chain["audit_count"],
            "audit_majority": chain["audit_majority"],
            "missing_commits": chain["missing_commits"],
            "missing_reveals": chain["missing_reveals"],
            "missing_audit_reports": chain["missing_audit_reports"],
            "gate": rec.get("gate"),
            "escrow": rec.get("escrow"),
            "comparison": rec.get("comparison"),
            "jurors": jurors,
        })
    return {"run_dir": run_dir.name, "mode": run["mode"], "unix_time": run.get("unix_time"), "counts": run["counts"], "cases": cases}


def main():
    run_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else sorted((ROOT / "runs").iterdir())[-1]
    data = load(run_dir)
    template = (ROOT / "viewer" / "template.html").read_text()
    blob = json.dumps(data, ensure_ascii=False).replace("</", "<\\/")
    out = ROOT / "viewer" / "index.html"
    out.write_text(template.replace("/*__DATA__*/null", blob))
    n = sum(1 for c in data["cases"] for j in c["jurors"] if j["verified"])
    print(f"wrote {out.relative_to(ROOT)}: {len(data['cases'])} cases, {n} commitments verified")


if __name__ == "__main__":
    main()

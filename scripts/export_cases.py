"""Export recorded panels from the Apart run logs into case files.

Usage: python3 scripts/export_cases.py <apart-repo> [run-label] [condition]

Each case file holds the first read of each juror and, where the log has
one, the read after discussion, in basis points. The numbers are model outputs from the logs. This
script only normalizes each read so that it sums to 10,000.
"""
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path

BPS = 10_000
POLICY = (
    "Protocol 2. Equal weight for each juror. Sum the revealed first reads for each option. "
    "An option is decided when its sum is more than 5000 times the number of jurors. "
    "If no option is decided, the outcome is INCONCLUSIVE. "
    "If a commit or a reveal is missing, the outcome is INCOMPLETE."
)


def to_bps(probs, names):
    vals = [max(float(probs.get(n, 0.0)), 0.0) for n in names]
    total = sum(vals)
    if total <= 0:
        return None
    out = [round(v / total * BPS) for v in vals]
    out[out.index(max(out))] += BPS - sum(out)
    return out


def main():
    repo = Path(sys.argv[1])
    label = sys.argv[2] if len(sys.argv) > 2 else "pushJ"
    cond = sys.argv[3] if len(sys.argv) > 3 else "C2"
    scenarios = {s["id"]: s for s in json.loads((repo / "runs" / "scenarios.json").read_text())}
    panels = defaultdict(lambda: defaultdict(dict))
    with open(repo / "runs" / f"{label}.jsonl") as fh:
        for line in fh:
            r = json.loads(line)
            if r.get("cond") != cond or r.get("stage") not in ("initial", "final"):
                continue
            key = (r["scenario_id"], int(r["seed"]), r["arm"])
            panels[key][int(r["agent_idx"])][r["stage"]] = (r.get("model"), r.get("probs"), r.get("raw") or "")
    out_dir = Path(__file__).resolve().parent.parent / "cases" / label
    out_dir.mkdir(parents=True, exist_ok=True)
    kept = skipped = 0
    for (sid, seed, arm), agents in sorted(panels.items()):
        sc = scenarios[sid]
        names = [s["name"] for s in sc["suspects"]]
        jurors = []
        for idx in sorted(agents):
            a = agents[idx]
            if "initial" not in a or not a["initial"][1]:
                continue
            first = to_bps(a["initial"][1], names)
            if first is None:
                continue
            # A juror with no usable read after the discussion posts no final read.
            final = to_bps(a["final"][1], names) if a.get("final") and a["final"][1] else None
            jurors.append({
                "agent_idx": idx,
                "model": a["initial"][0],
                "first": first,
                # Hash of the raw first-read output of the model, as logged.
                "rationale_hash": hashlib.sha256(a["initial"][2].encode()).hexdigest(),
                "final": final,
            })
        if len(jurors) != len(agents) or len(jurors) < 2:
            skipped += 1
            continue
        evidence = json.dumps(sc, sort_keys=True).encode()
        case = {
            "job_id": f"{label}-{sid}-s{seed}",
            "case_key": f"apart-swarm-alignment:{label}:{sid}:seed{seed}:{cond}",
            "fixture_type": "recorded puzzle panel",
            "fixture_note": "A whodunit puzzle with a known answer. It is not a market dispute.",
            "title": sc["title"],
            "source": f"apart-swarm-alignment runs/{label}.jsonl, arm {arm}, condition {cond}, seed {seed}",
            "evidence_hash": hashlib.sha256(evidence).hexdigest(),
            "policy": POLICY,
            "policy_hash": hashlib.sha256(POLICY.encode()).hexdigest(),
            "options": names,
            "truth": names.index(sc["ground_truth"]),
            "jurors": jurors,
        }
        (out_dir / f"{sid}-s{seed}.json").write_text(json.dumps(case, indent=1) + "\n")
        kept += 1
    print(f"wrote {kept} cases to {out_dir}; skipped {skipped} panels with a missing first read")


if __name__ == "__main__":
    main()

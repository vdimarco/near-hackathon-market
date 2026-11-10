"""Collect public dispute records from the legacy NEAR Agent Market site.

Usage: python3 scripts/fetch_market_cases.py [cache-dir]

The legacy site (market-legacy.near.ai) shows a public page for each job. A
page of a disputed job shows the job description, the public messages, the
dispute reason, and the ruling. It does not show the submitted work. So each
record is a PARTIAL PUBLIC CASE. It shows the dispute workflow. It cannot
support a ruling on the merits.

The script reads job ids from the public search page, loads each job page
once with a short wait, and keeps pages in the cache directory.
"""
import hashlib
import html
import json
import re
import sys
import time
import urllib.parse
import urllib.request
from collections import Counter
from datetime import date
from pathlib import Path

BASE = "https://market-legacy.near.ai"
SEARCHES = [
    "dispute", "RELISTED", "Build", "Create", "MoltBook", "Write", "v4", "v3", "v2", "agent", "NEAR", "blog",
    "research", "security", "review", "test", "tool", "data", "python", "marketplace", "rejected", "redo",
    "deliverable", "docs", "tutorial",
]


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "lock-jury-fixture-fetch/0.1"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8", "replace")


def page_text(raw):
    body = raw[raw.find("<body"):]
    body = re.sub(r"<script.*?</script>|<style.*?</style>", "", body, flags=re.S)
    text = html.unescape(re.sub(r"<[^>]+>", "\n", body))
    return re.sub(r"\n\s*\n+", "\n", text)


def clean(value):
    return re.sub(r"[☆★\s]+", " ", value).strip() if value else None


def field(pattern, text):
    m = re.search(pattern, text, flags=re.S)
    return clean(m.group(1)) if m else None


def parse(job_id, raw):
    text = page_text(raw)
    if "Dispute opened by" not in text:
        return None
    dispute = text[text.rfind("\nDispute\nOpened by\n"):]
    description = field(r"\nDescription\n(.*?)\nCreator\n", text) or ""
    title = field(r"<title>(.*?) - Agent Market</title>", raw)
    return {
        "source_market": "market-legacy.near.ai",
        "job_id": job_id,
        "url": f"{BASE}/jobs/{job_id}",
        "title": html.unescape(title) if title else None,
        "fixture_type": "partial public case",
        "submission": "not available to this review",
        "description_sha256": hashlib.sha256(description.encode()).hexdigest(),
        "description_chars": len(description),
        "creator": field(r"\nCreator\n(.*?)\n", text),
        "dispute_opened_by": field(r"Opened by\n(.*?)\n", dispute),
        "dispute_reason": field(r"Opened by\n.*?\n[☆★\n]*(.*?)\nRuling\n", dispute),
        # Comparison record. Do not show these two fields to a juror.
        "market_ruling": field(r"\nRuling\n(.*?)\n", dispute),
        "resolved_by": field(r"Resolved by\n(.*?)\n", dispute),
    }


def main():
    cache = Path(sys.argv[1] if len(sys.argv) > 1 else ".market-cache")
    (cache / "jobs").mkdir(parents=True, exist_ok=True)
    ids_file = cache / "ids.txt"
    if ids_file.exists():
        ids = ids_file.read_text().split()
    else:
        found = set()
        for q in SEARCHES:
            raw = get(f"{BASE}/jobs?search={urllib.parse.quote(q)}")
            found.update(re.findall(r"window\.location='/jobs/([0-9a-f-]{36})'", raw))
            time.sleep(0.4)
        ids = sorted(found)
        ids_file.write_text("\n".join(ids) + "\n")
    records = []
    for job_id in ids:
        path = cache / "jobs" / f"{job_id}.html"
        if not path.exists():
            path.write_text(get(f"{BASE}/jobs/{job_id}"))
            time.sleep(0.3)
        rec = parse(job_id, path.read_text())
        if rec:
            records.append(rec)
    rulings = Counter(r["market_ruling"] for r in records)
    with_resolver = [r for r in records if r["resolved_by"]]
    summary = {
        "retrieved": date.today().isoformat(),
        "source": "public job pages of market-legacy.near.ai (the legacy market, not the current market)",
        "job_pages_read": len(ids),
        "disputed_jobs": len(records),
        "market_rulings": dict(rulings.most_common()),
        "dispute_reasons": dict(Counter(r["dispute_reason"] for r in records).most_common(3)),
        "pages_that_show_a_resolver": len(with_resolver),
        "resolver_is_also_job_creator": sum(1 for r in with_resolver if r["resolved_by"] == r["creator"]),
        "note": (
            "The job ids come from keyword searches, so this is not a complete or random sample. "
            "No page shows the submitted work. A page that shows one account as creator and resolver "
            "shows overlapping roles. It does not show whether the ruling was fair."
        ),
    }
    out = Path(__file__).resolve().parent.parent / "cases" / "market-legacy"
    out.mkdir(parents=True, exist_ok=True)
    (out / "index.json").write_text(json.dumps({"summary": summary, "cases": records}, indent=1) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()

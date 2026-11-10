//! Recorded replay of jury panels on a local NEAR sandbox.
//!
//! Usage: cargo run -p demo -- [cases-dir] [--limit N]
//! With no --limit the program replays every case file in the directory.
//!
//! The program needs no API key. It exits with an error if a result differs
//! from the expected result.

use anyhow::{anyhow, bail, Context, Result};
use demo::*;
use futures::future::try_join_all;
use near_workspaces::network::Sandbox;
use near_workspaces::types::NearToken;
use near_workspaces::{Account, Contract, Worker};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Deserialize)]
struct JurorRead {
    model: String,
    first: Vec<u32>,
    rationale_hash: String,
    #[serde(rename = "final")]
    final_read: Option<Vec<u32>>,
}

#[derive(Deserialize)]
struct CaseFile {
    job_id: String,
    case_key: String,
    fixture_type: String,
    evidence_hash: String,
    policy_hash: String,
    options: Vec<String>,
    truth: usize,
    jurors: Vec<JurorRead>,
}

/// The replay uses a salt made from the case and the juror, so that each run
/// prints the same hashes. A live juror must use 32 random bytes.
fn replay_salt(case_key: &str, juror: &str) -> String {
    sha_hex(format!("replay-salt:{}:{}", case_key, juror).as_bytes())
}

fn label(options: &[String], pick: Option<usize>, truth: usize, empty: &str) -> String {
    match pick {
        Some(i) if i == truth => options[i].clone(),
        Some(i) => format!("{} (wrong)", options[i]),
        None => empty.to_string(),
    }
}

/// Returns true if the node or the contract refused the call.
async fn refused(account: &Account, contract: &Contract, method: &str, args: Value) -> bool {
    match account.call(contract.id(), method).args_json(args).transact().await {
        Err(_) => true,
        Ok(r) => r.is_failure(),
    }
}

fn expect(ok: bool, what: &str) -> Result<()> {
    println!("   {:<66} {}", what, if ok { "ok" } else { "FAILED" });
    if ok { Ok(()) } else { bail!("expected result differs: {}", what) }
}

/// Commit, post "no report", and return the salts. The case is then in the reveal phase.
async fn commit_and_skip_audit(panel: &Panel, id: u64, evidence: &str, policy: &str, reads: &[Vec<u32>], rationale: &str) -> Result<Vec<String>> {
    let n = panel.jurors.len();
    let salts: Vec<String> = panel.ids.iter().map(|j| replay_salt(&format!("synthetic:{}", id), j)).collect();
    try_join_all((0..n).map(|j| {
        let h = commit_hash(panel, id, evidence, policy, j, &reads[j], rationale, &salts[j]);
        panel.jurors[j].call(panel.contract.id(), "commit").args_json(json!({ "case_id": id, "hash": h })).transact()
    }))
    .await?
    .into_iter()
    .try_for_each(|r| r.into_result().map(|_| ()))?;
    try_join_all((0..n).map(|j| {
        panel.jurors[j].call(panel.contract.id(), "final_read").args_json(json!({ "case_id": id, "probs": null })).transact()
    }))
    .await?
    .into_iter()
    .try_for_each(|r| r.into_result().map(|_| ()))?;
    Ok(salts)
}

async fn reveal(panel: &Panel, id: u64, j: usize, probs: &[u32], rationale: &str, salt: &str) -> Result<bool> {
    let r = panel.jurors[j]
        .call(panel.contract.id(), "reveal")
        .args_json(json!({ "case_id": id, "probs": probs, "rationale_hash": rationale, "salt": salt }))
        .transact()
        .await?;
    Ok(r.is_success())
}

async fn yocto(account_id: &near_workspaces::AccountId, worker: &Worker<Sandbox>) -> Result<u128> {
    Ok(worker.view_account(account_id).await?.balance.as_yoctonear())
}

async fn escrow_tests(panel: &Panel, worker: &Worker<Sandbox>, deals: [u64; 3], decided_case: u64) -> Result<()> {
    let two = NearToken::from_near(2).as_yoctonear();
    let [inconclusive, incomplete, decided] = deals;
    let worker_before = yocto(panel.worker.id(), worker).await?;
    let buyer_before = yocto(panel.buyer.id(), worker).await?;
    let escrow_before = yocto(panel.escrow.id(), worker).await?;

    settle(panel, inconclusive).await?;
    let d = deal_view(panel, inconclusive).await?;
    expect(d["status"] == "Held" && d["last_jury_outcome"] == "Inconclusive", "INCONCLUSIVE case: settle leaves 2 NEAR held")?;
    settle(panel, incomplete).await?;
    let d = deal_view(panel, incomplete).await?;
    expect(d["status"] == "Held" && d["last_jury_outcome"] == "Incomplete", "INCOMPLETE case: settle leaves 2 NEAR held")?;
    expect(yocto(panel.worker.id(), worker).await? == worker_before, "worker balance did not change")?;

    settle(panel, decided).await?;
    let d = deal_view(panel, decided).await?;
    expect(d["status"] == "PaidWorker", "DECIDED worker wins: deal status is PaidWorker")?;
    expect(yocto(panel.worker.id(), worker).await? == worker_before + two, "worker received exactly 2 NEAR")?;
    expect(!settle(panel, decided).await?, "second settle on the paid deal: refused")?;
    expect(yocto(panel.worker.id(), worker).await? == worker_before + two, "worker balance is still plus 2 NEAR after the retry")?;

    // A case where the buyer wins.
    let options: Vec<String> = RULINGS.iter().map(|s| s.to_string()).collect();
    let (evidence, policy, rationale) = (sha_hex(b"synthetic evidence"), sha_hex(b"synthetic policy"), sha_hex(b"synthetic rationale"));
    let buyer_reads: Vec<Vec<u32>> = (0..6).map(|_| vec![1000, 1000, 8000]).collect();
    let case = open_case(panel, "synthetic:requester-panel", &evidence, &policy, &options, 600).await?;
    let refund_deal = fund(panel, case, panel.worker.id().as_str()).await?;
    let salts = commit_and_skip_audit(panel, case, &evidence, &policy, &buyer_reads, &rationale).await?;
    for j in 0..6 {
        reveal(panel, case, j, &buyer_reads[j], &rationale, &salts[j]).await?;
    }
    settle(panel, refund_deal).await?;
    let d = deal_view(panel, refund_deal).await?;
    expect(d["status"] == "RefundedRequester", "DECIDED requester wins: deal status is RefundedRequester")?;
    expect(yocto(panel.buyer.id(), worker).await? == buyer_before + two, "buyer received exactly 2 NEAR")?;

    // A transfer that fails. The worker account of this deal does not exist yet.
    let ghost_id = format!("ghost.{}", panel.root.id());
    let ghost_deal = fund(panel, decided_case, &ghost_id).await?;
    settle(panel, ghost_deal).await?;
    let d = deal_view(panel, ghost_deal).await?;
    expect(d["status"] == "Held" && d["failed_transfers"] == 1, "transfer to a missing account fails: deal returns to Held")?;
    let ghost = panel.root.create_subaccount("ghost").initial_balance(NearToken::from_near(1)).transact().await?.into_result()?;
    let ghost_before = yocto(ghost.id(), worker).await?;
    settle(panel, ghost_deal).await?;
    let d = deal_view(panel, ghost_deal).await?;
    expect(d["status"] == "PaidWorker" && d["settle_calls"] == 2, "retry after the account exists: PaidWorker")?;
    expect(yocto(ghost.id(), worker).await? == ghost_before + two, "that worker received exactly 2 NEAR, one time")?;

    // 5 deals of 2 NEAR came in. 3 were paid out. 2 are held.
    let escrow_after = yocto(panel.escrow.id(), worker).await?;
    let held_change = escrow_after as i128 - escrow_before as i128;
    let near = NearToken::from_near(1).as_yoctonear() as i128;
    expect((held_change - (-2 * near)).abs() < near / 10, "escrow balance: 3 deals paid out, 2 deals still held")?;
    Ok(())
}

async fn attack_tests(panel: &Panel) -> Result<([u64; 3], u64)> {
    let options: Vec<String> = RULINGS.iter().map(|s| s.to_string()).collect();
    let evidence = sha_hex(b"synthetic evidence");
    let policy = sha_hex(b"synthetic policy");
    let rationale = sha_hex(b"synthetic rationale");
    let split: Vec<Vec<u32>> = (0..6).map(|j| if j < 3 { vec![9000, 500, 500] } else { vec![500, 500, 9000] }).collect();
    let worker: Vec<Vec<u32>> = (0..6).map(|_| vec![7000, 2000, 1000]).collect();

    // A. The complete split panel.
    let a = open_case(panel, "synthetic:split-panel:complete", &evidence, &policy, &options, 600).await?;
    let deal_a = fund(panel, a, panel.worker.id().as_str()).await?;
    let salts = commit_and_skip_audit(panel, a, &evidence, &policy, &split, &rationale).await?;
    for j in 0..6 {
        reveal(panel, a, j, &split[j], &rationale, &salts[j]).await?;
    }
    let v = view_case(panel, a).await?;
    expect(v["outcome"] == "Inconclusive" && v["ruling"].is_null(), "split panel, all 6 reveal: INCONCLUSIVE, no ruling")?;

    // B. The same panel. One juror that favors the requester withholds its reveal.
    let b = open_case(panel, "synthetic:split-panel:one-withholds", &evidence, &policy, &options, 8).await?;
    let deal_b = fund(panel, b, panel.worker.id().as_str()).await?;
    let salts = commit_and_skip_audit(panel, b, &evidence, &policy, &split, &rationale).await?;
    for j in 0..5 {
        reveal(panel, b, j, &split[j], &rationale, &salts[j]).await?;
    }
    let five: Vec<&Vec<u32>> = split[..5].iter().collect();
    let naive = strict_majority(&offline_sums(&five, 3), 5);
    expect(naive == Some(0), "offline: a pool of the 5 reveals would say 'worker wins' (56%)")?;
    let early = refused(&panel.requester, &panel.contract, "advance", json!({ "case_id": b })).await;
    expect(early, "advance before the reveal deadline: refused")?;
    let mut closed = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if !refused(&panel.requester, &panel.contract, "advance", json!({ "case_id": b })).await {
            closed = true;
            break;
        }
    }
    let v = view_case(panel, b).await?;
    expect(
        closed && v["outcome"] == "Incomplete" && v["ruling"].is_null() && v["missing_reveals"] == json!([panel.ids[5]]),
        "one reveal withheld: INCOMPLETE, no ruling, j6 recorded as missing",
    )?;

    // C. Changed values at reveal, and repeated calls.
    let c = open_case(panel, "synthetic:worker-panel:tamper", &evidence, &policy, &options, 600).await?;
    let deal_c = fund(panel, c, panel.worker.id().as_str()).await?;
    let again = refused(&panel.jurors[0], &panel.contract, "final_read", json!({ "case_id": c, "probs": null })).await;
    expect(again, "audit report before the discussion phase: refused")?;
    let salts = commit_and_skip_audit(panel, c, &evidence, &policy, &worker, &rationale).await?;
    let dup = refused(&panel.jurors[0], &panel.contract, "commit", json!({ "case_id": c, "hash": "a".repeat(64) })).await;
    expect(dup, "second commit from one juror: refused")?;
    expect(!reveal(panel, c, 0, &[1000, 2000, 7000], &rationale, &salts[0]).await?, "reveal with changed scores: refused")?;
    expect(!reveal(panel, c, 0, &worker[0], &rationale, &salts[1]).await?, "reveal with a different salt: refused")?;
    expect(!reveal(panel, c, 0, &worker[0], &sha_hex(b"other"), &salts[0]).await?, "reveal with a changed rationale hash: refused")?;
    for j in 0..6 {
        reveal(panel, c, j, &worker[j], &rationale, &salts[j]).await?;
    }
    expect(!reveal(panel, c, 0, &worker[0], &rationale, &salts[0]).await?, "second reveal from one juror: refused")?;
    let v = view_case(panel, c).await?;
    expect(v["outcome"] == "Decided" && v["ruling"] == 0, "correct reveals from all 6: DECIDED, worker wins")?;
    Ok(([deal_a, deal_b, deal_c], c))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dir = PathBuf::from("cases/pushJ");
    let mut limit = usize::MAX;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                i += 1;
                limit = args.get(i).context("--limit needs a number")?.parse()?;
            }
            other => dir = PathBuf::from(other),
        }
        i += 1;
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    paths.sort();
    paths.truncate(limit);
    if paths.is_empty() {
        bail!("no case files in {}", dir.display());
    }
    let cases: Vec<CaseFile> = paths.iter().map(|p| Ok(serde_json::from_str(&std::fs::read_to_string(p)?)?)).collect::<Result<_>>()?;
    let n_jurors = cases[0].jurors.len();
    if n_jurors != 6 || cases.iter().any(|c| c.jurors.len() != n_jurors) {
        bail!("this demo needs 6 jurors in each case");
    }

    println!("MODE: RECORDED REPLAY on a local sandbox. Operator-run panel: one operator holds all juror keys.");
    println!("This mode shows that the contract accepts valid inputs, refuses invalid inputs, and computes");
    println!("the expected outcome. The model calls and the discussion happened before this run. This mode");
    println!("does not show when they happened.");
    println!();
    println!("== 1. Start the sandbox, deploy the jury and escrow contracts, create 6 juror accounts");
    let worker = near_workspaces::sandbox().await?;
    let panel = setup(&worker, n_jurors).await?;
    println!("   jury:     {}  (protocol {})", panel.contract.id(), PROTOCOL_VERSION);
    println!("   escrow:   {}", panel.escrow.id());
    println!("   jurors:   {}", panel.ids.join(", "));

    println!("== 2. Juror key limits");
    let transfer = match panel.jurors[0].transfer_near(panel.requester.id(), NearToken::from_near(1)).await {
        Err(e) => {
            println!("   node: {}", first_line(&format!("{:?}", e)));
            true
        }
        Ok(r) => r.is_failure(),
    };
    expect(transfer, "juror key sends 1 NEAR to another account: refused")?;
    let open = panel.jurors[0]
        .call(panel.contract.id(), "open_case")
        .args_json(json!({ "case_key": "k", "evidence_hash": "0".repeat(64), "policy_hash": "0".repeat(64),
            "options": ["a", "b"], "commit_secs": 60, "audit_secs": 60, "reveal_secs": 60 }))
        .transact()
        .await;
    let open_refused = match open {
        Err(e) => {
            println!("   node: {}", first_line(&format!("{:?}", e)));
            true
        }
        Ok(r) => r.is_failure(),
    };
    expect(open_refused, "juror key calls open_case: refused")?;

    println!("== 3. Attack tests on synthetic cases (fixture type: synthetic, expected outcome stated)");
    let (deals, decided_case) = attack_tests(&panel).await?;

    println!("== 4. Sandbox escrow: payment follows the jury outcome (each deal holds 2 NEAR)");
    escrow_tests(&panel, &worker, deals, decided_case).await?;

    println!("== 5. Replay {} recorded panels (fixture type: {})", cases.len(), cases[0].fixture_type);
    println!("   {:<14} {:<7} {:<13} {:<15} {:<15} {:>6}  {}", "case", "truth", "outcome", "first reads", "after talk", "moved", "audit reports");
    let (mut decided_ok, mut decided_wrong, mut inconclusive) = (0, 0, 0);
    let (mut first_ok, mut audit_ok, mut audit_wrong) = (0, 0, 0);
    let (mut same_first_ok, mut same_audit_ok) = (0, 0);
    for case in &cases {
        let k = case.options.len();
        let id = open_case(&panel, &case.case_key, &case.evidence_hash, &case.policy_hash, &case.options, 600).await?;
        let salts: Vec<String> = panel.ids.iter().map(|j| replay_salt(&case.case_key, j)).collect();
        try_join_all((0..n_jurors).map(|j| {
            let r = &case.jurors[j];
            let h = commit_hash(&panel, id, &case.evidence_hash, &case.policy_hash, j, &r.first, &r.rationale_hash, &salts[j]);
            panel.jurors[j].call(panel.contract.id(), "commit").args_json(json!({ "case_id": id, "hash": h })).transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;
        let open: bool = panel.contract.view("is_discussion_open").args_json(json!({ "case_id": id })).await?.json()?;
        if !open {
            bail!("the discussion did not open after 6 commits: {}", case.job_id);
        }
        // The recorded discussion goes here. Then each juror posts its audit report.
        try_join_all((0..n_jurors).map(|j| {
            panel.jurors[j]
                .call(panel.contract.id(), "final_read")
                .args_json(json!({ "case_id": id, "probs": case.jurors[j].final_read }))
                .transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;
        try_join_all((0..n_jurors).map(|j| {
            let r = &case.jurors[j];
            panel.jurors[j]
                .call(panel.contract.id(), "reveal")
                .args_json(json!({ "case_id": id, "probs": r.first, "rationale_hash": r.rationale_hash, "salt": salts[j] }))
                .transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;
        let view = view_case(&panel, id).await?;

        // The contract result must match a plain offline calculation.
        let firsts: Vec<&Vec<u32>> = case.jurors.iter().map(|j| &j.first).collect();
        let off_sums = offline_sums(&firsts, k);
        let off_ruling = strict_majority(&off_sums, n_jurors as u32);
        let off_outcome = if off_ruling.is_some() { "Decided" } else { "Inconclusive" };
        let chain_sums = u32s(&view["first_sums"]).ok_or_else(|| anyhow!("no first_sums"))?;
        let chain_ruling = view["ruling"].as_u64().map(|x| x as u32);
        if chain_sums != off_sums || chain_ruling != off_ruling || view["outcome"] != off_outcome {
            bail!("contract result differs from the offline calculation: {}", case.job_id);
        }

        let audits: Vec<&Vec<u32>> = case.jurors.iter().filter_map(|j| j.final_read.as_ref()).collect();
        let audit_top = if audits.is_empty() { None } else { top(&offline_sums(&audits, k)) };
        let both: Vec<&JurorRead> = case.jurors.iter().filter(|j| j.final_read.is_some()).collect();
        let same_first: Vec<&Vec<u32>> = both.iter().map(|j| &j.first).collect();
        if !both.is_empty() && top(&offline_sums(&same_first, k)) == Some(case.truth) {
            same_first_ok += 1;
        }
        if audit_top == Some(case.truth) {
            same_audit_ok += 1;
            audit_ok += 1;
        } else if audit_top.is_some() {
            audit_wrong += 1;
        }
        let first_top = top(&off_sums);
        if first_top == Some(case.truth) {
            first_ok += 1;
        }
        match chain_ruling {
            Some(r) if r as usize == case.truth => decided_ok += 1,
            Some(_) => decided_wrong += 1,
            None => inconclusive += 1,
        }
        let moved: Vec<u64> = view["jurors"].as_array().map(|a| a.iter().filter_map(|j| j["moved_bps"].as_u64()).collect()).unwrap_or_default();
        let mean_moved = if moved.is_empty() { 0.0 } else { moved.iter().sum::<u64>() as f64 / moved.len() as f64 / 100.0 };
        println!(
            "   {:<14} {:<7} {:<13} {:<15} {:<15} {:>5.1}%  {}/{}",
            case.job_id,
            case.options[case.truth],
            view["outcome"].as_str().unwrap_or("?").to_uppercase(),
            label(&case.options, first_top, case.truth, "tie"),
            label(&case.options, audit_top, case.truth, "tie"),
            mean_moved,
            both.len(),
            n_jurors
        );
    }

    let n = cases.len();
    println!("== 6. Result of the recorded replay");
    println!("   contract result equals the offline calculation:    {} of {}", n, n);
    println!("   contract outcome: {} decided and correct, {} decided and wrong, {} inconclusive", decided_ok, decided_wrong, inconclusive);
    println!("   top option of the first reads is correct:          {} of {}", first_ok, n);
    println!("   top option of the audit reads:                     {} correct, {} wrong, {} tie", audit_ok, audit_wrong, n - audit_ok - audit_wrong);
    println!("   same jurors at both stages, first reads correct:   {} of {}", same_first_ok, n);
    println!("   same jurors at both stages, audit reads correct:   {} of {}", same_audit_ok, n);
    println!("   models: {}", cases[0].jurors.iter().map(|j| j.model.clone()).collect::<Vec<_>>().join(", "));
    println!("ALL EXPECTED RESULTS MATCH");
    Ok(())
}

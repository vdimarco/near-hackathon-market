//! Replay recorded jury panels on a local NEAR sandbox.
//!
//! Usage: cargo run -p demo -- [cases-dir] [--limit N]
//! With no --limit the program replays every case file in the directory.
//!
//! The program needs no API key. It starts a sandbox, deploys the jury
//! contract, gives each juror one key that can make three calls, and sends
//! each recorded read through commit, reveal, and final_read.

use anyhow::{anyhow, bail, Context, Result};
use futures::future::try_join_all;
use near_workspaces::types::{AccessKey, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, Contract};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const JUROR_METHODS: [&str; 3] = ["commit", "reveal", "final_read"];

#[derive(Deserialize)]
struct JurorRead {
    model: String,
    first: Vec<u32>,
    #[serde(rename = "final")]
    final_read: Option<Vec<u32>>,
}

#[derive(Deserialize)]
struct CaseFile {
    job_id: String,
    evidence_hash: String,
    options: Vec<String>,
    truth: usize,
    jurors: Vec<JurorRead>,
}

fn sha_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect()
}

/// The replay uses a salt made from the case and the juror, so that each run
/// prints the same hashes. A live juror must use 32 random bytes.
fn replay_salt(job_id: &str, juror: &str) -> String {
    sha_hex(&format!("replay-salt:{}:{}", job_id, juror))
}

fn commit_hash(case_id: u64, juror: &str, probs: &[u32], salt: &str) -> String {
    let joined: Vec<String> = probs.iter().map(|p| p.to_string()).collect();
    sha_hex(&format!("{}:{}:{}:{}", case_id, juror, joined.join(","), salt))
}

/// Build the contract with plain cargo. No extra build tool is needed.
fn build_contract() -> Result<Vec<u8>> {
    let status = std::process::Command::new("cargo")
        .args(["build", "-q", "-p", "lock-jury", "--target", "wasm32-unknown-unknown", "--release"])
        .status()
        .context("cannot run cargo")?;
    if !status.success() {
        bail!("contract build failed. Run: rustup target add wasm32-unknown-unknown");
    }
    let path = "target/wasm32-unknown-unknown/release/lock_jury.wasm";
    std::fs::read(path).with_context(|| format!("cannot read {}", path))
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").chars().take(160).collect()
}

/// The single top option of a pooled read. `None` when there is a tie or no read.
fn top(pooled: &Value) -> Option<usize> {
    let v: Vec<u64> = pooled.as_array()?.iter().filter_map(|x| x.as_u64()).collect();
    let max = *v.iter().max()?;
    let tops: Vec<usize> = (0..v.len()).filter(|i| v[*i] == max).collect();
    if tops.len() == 1 { Some(tops[0]) } else { None }
}

fn top_of_reads(reads: &[&Value], n_options: usize) -> Option<usize> {
    let mut sums = vec![0u64; n_options];
    for read in reads {
        for (i, p) in read.as_array()?.iter().enumerate() {
            sums[i] += p.as_u64()?;
        }
    }
    top(&json!(sums))
}

fn label(options: &[String], pick: Option<usize>, truth: usize, has_read: bool) -> String {
    match pick {
        Some(i) if i == truth => options[i].clone(),
        Some(i) => format!("{} (wrong)", options[i]),
        None if has_read => "tie".to_string(),
        None => "no read".to_string(),
    }
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
    let cases: Vec<CaseFile> = paths
        .iter()
        .map(|p| Ok(serde_json::from_str(&std::fs::read_to_string(p)?)?))
        .collect::<Result<_>>()?;
    let panel = cases[0].jurors.len();
    if cases.iter().any(|c| c.jurors.len() != panel) {
        bail!("all cases must have the same number of jurors");
    }

    println!("== 1. Start the sandbox and deploy the contract");
    let worker = near_workspaces::sandbox().await?;
    let wasm = build_contract()?;
    let root = worker.root_account()?;
    let jury_account = root
        .create_subaccount("jury")
        .initial_balance(NearToken::from_near(40))
        .transact()
        .await?
        .into_result()?;
    let requester = root
        .create_subaccount("requester")
        .initial_balance(NearToken::from_near(10))
        .transact()
        .await?
        .into_result()?;

    println!("== 2. Create {} juror accounts. Each one keeps a single key for 3 methods", panel);
    let mut jurors: Vec<Account> = Vec::new();
    for j in 0..panel {
        let full = jury_account
            .create_subaccount(&format!("j{}", j + 1))
            .initial_balance(NearToken::from_near(3))
            .transact()
            .await?
            .into_result()?;
        let limited_key = SecretKey::from_random(KeyType::ED25519);
        full.batch(full.id())
            .add_key(
                limited_key.public_key(),
                AccessKey::function_call_access(jury_account.id(), &JUROR_METHODS, None),
            )
            .delete_key(full.secret_key().public_key())
            .transact()
            .await?
            .into_result()?;
        jurors.push(Account::from_secret_key(full.id().clone(), limited_key, &worker));
    }
    let juror_ids: Vec<String> = jurors.iter().map(|a| a.id().to_string()).collect();

    let contract: Contract = jury_account.deploy(&wasm).await?.into_result()?;
    contract
        .call("new")
        .args_json(json!({ "jurors": juror_ids }))
        .transact()
        .await?
        .into_result()?;
    println!("   contract: {}", contract.id());
    println!("   jurors:   {}", juror_ids.join(", "));

    println!("== 3. Check the juror key limits");
    let transfer = jurors[0].transfer_near(requester.id(), NearToken::from_near(1)).await;
    let transfer_blocked = match transfer {
        Err(e) => {
            println!("   node answer to the transfer: {}", first_line(&format!("{:?}", e)));
            true
        }
        Ok(r) => r.is_failure(),
    };
    let open = jurors[0]
        .call(contract.id(), "open_case")
        .args_json(json!({
            "job_id": "juror-key-test", "evidence_hash": "0".repeat(64),
            "options": ["a", "b"], "default_option": null, "commit_secs": 60, "reveal_secs": 60
        }))
        .transact()
        .await;
    let open_blocked = match open {
        Err(e) => {
            println!("   node answer to open_case:    {}", first_line(&format!("{:?}", e)));
            true
        }
        Ok(r) => r.is_failure(),
    };
    println!("   juror key sends 1 NEAR to another account: {}", if transfer_blocked { "blocked" } else { "ALLOWED" });
    println!("   juror key calls open_case:                 {}", if open_blocked { "blocked" } else { "ALLOWED" });
    if !transfer_blocked || !open_blocked {
        bail!("a juror key did more than the 3 juror methods");
    }

    println!("== 4. Replay {} recorded panels", cases.len());
    println!("   {:<16} {:<8} {:<16} {:<16} {:>9}  {}", "case", "truth", "locked reads", "after talk", "moved", "reads after talk");
    let (mut first_ok, mut final_ok, mut final_wrong, mut clear_ok, mut early_reveal_blocked) = (0, 0, 0, 0, 0);
    let (mut same_first_ok, mut same_final_ok) = (0, 0);
    for case in &cases {
        let id: u64 = requester
            .call(contract.id(), "open_case")
            .args_json(json!({
                "job_id": case.job_id, "evidence_hash": case.evidence_hash,
                "options": case.options, "default_option": null,
                "commit_secs": 600, "reveal_secs": 600
            }))
            .transact()
            .await?
            .into_result()?
            .json()?;
        let salts: Vec<String> = juror_ids.iter().map(|j| replay_salt(&case.job_id, j)).collect();

        // The first juror commits, then tries to reveal while commits are open.
        let h0 = commit_hash(id, &juror_ids[0], &case.jurors[0].first, &salts[0]);
        jurors[0].call(contract.id(), "commit").args_json(json!({ "case_id": id, "hash": h0 })).transact().await?.into_result()?;
        let early = jurors[0]
            .call(contract.id(), "reveal")
            .args_json(json!({ "case_id": id, "probs": case.jurors[0].first, "salt": salts[0] }))
            .transact()
            .await?;
        if early.is_failure() {
            early_reveal_blocked += 1;
        }
        let open_before: bool = contract.view("is_discussion_open").args_json(json!({ "case_id": id })).await?.json()?;

        try_join_all((1..panel).map(|j| {
            let h = commit_hash(id, &juror_ids[j], &case.jurors[j].first, &salts[j]);
            jurors[j].call(contract.id(), "commit").args_json(json!({ "case_id": id, "hash": h })).transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;
        let open_after: bool = contract.view("is_discussion_open").args_json(json!({ "case_id": id })).await?.json()?;
        if open_before || !open_after {
            bail!("discussion gate is wrong for case {}", case.job_id);
        }

        // The recorded discussion happened here, after the last commit.

        try_join_all((0..panel).map(|j| {
            jurors[j]
                .call(contract.id(), "reveal")
                .args_json(json!({ "case_id": id, "probs": case.jurors[j].first, "salt": salts[j] }))
                .transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;

        try_join_all((0..panel).filter(|j| case.jurors[*j].final_read.is_some()).map(|j| {
            jurors[j]
                .call(contract.id(), "final_read")
                .args_json(json!({ "case_id": id, "probs": case.jurors[j].final_read }))
                .transact()
        }))
        .await?
        .into_iter()
        .try_for_each(|r| r.into_result().map(|_| ()))?;

        requester.call(contract.id(), "finalize").args_json(json!({ "case_id": id })).transact().await?.into_result()?;
        let view: Value = contract.view("get_case").args_json(json!({ "case_id": id })).await?.json()?;

        let first_top = top(&view["pooled_first"]);
        let final_top = top(&view["pooled_final"]);
        let juror_views = view["jurors"].as_array().ok_or_else(|| anyhow!("no jurors in view"))?;
        let moved: Vec<u64> = juror_views.iter().filter_map(|j| j["moved_bps"].as_u64()).collect();
        let mean_moved = if moved.is_empty() { 0.0 } else { moved.iter().sum::<u64>() as f64 / moved.len() as f64 / 100.0 };
        // The same jurors at both stages: only jurors that posted a read after the talk.
        let both: Vec<&Value> = juror_views.iter().filter(|j| !j["final"].is_null()).collect();
        let same_first: Vec<&Value> = both.iter().map(|j| &j["first"]).collect();
        let same_final: Vec<&Value> = both.iter().map(|j| &j["final"]).collect();
        if top_of_reads(&same_first, case.options.len()) == Some(case.truth) {
            same_first_ok += 1;
        }
        if top_of_reads(&same_final, case.options.len()) == Some(case.truth) {
            same_final_ok += 1;
        }
        if first_top == Some(case.truth) {
            first_ok += 1;
        }
        match final_top {
            Some(t) if t == case.truth => final_ok += 1,
            Some(_) => final_wrong += 1,
            None => {}
        }
        if view["clear"].as_bool() == Some(true) && view["ruling"].as_u64() == Some(case.truth as u64) {
            clear_ok += 1;
        }
        println!(
            "   {:<16} {:<8} {:<16} {:<16} {:>8.1}%  {}/{}",
            case.job_id,
            case.options[case.truth],
            label(&case.options, first_top, case.truth, true),
            label(&case.options, final_top, case.truth, !both.is_empty()),
            mean_moved,
            both.len(),
            panel
        );
    }

    let n = cases.len();
    println!("== 5. Result");
    println!("   reveals before commits closed, blocked: {} of {}", early_reveal_blocked, n);
    println!("   top option of the locked first reads is correct: {} of {}", first_ok, n);
    println!(
        "   top option of the reads after talk:              {} correct, {} wrong, {} tie or no read",
        final_ok,
        final_wrong,
        n - final_ok - final_wrong
    );
    println!("   same jurors at both stages, first reads correct: {} of {}", same_first_ok, n);
    println!("   same jurors at both stages, after talk correct:  {} of {}", same_final_ok, n);
    println!("   contract ruling (one option above 50%) correct:  {} of {}", clear_ok, n);
    println!("   models: {}", cases[0].jurors.iter().map(|j| j.model.clone()).collect::<Vec<_>>().join(", "));
    if early_reveal_blocked != n {
        bail!("an early reveal was accepted");
    }
    Ok(())
}

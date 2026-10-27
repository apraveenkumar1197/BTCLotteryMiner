//! Minimal Stratum v1 client with auto-reconnect.

use crate::job::Job;
use crate::{Config, Shared, Submit};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedReceiver;

async fn send(w: &mut OwnedWriteHalf, v: Value) -> Result<()> {
    let mut line = v.to_string();
    line.push('\n');
    w.write_all(line.as_bytes()).await?;
    Ok(())
}

pub async fn run(cfg: Arc<Config>, shared: Arc<Shared>, mut rx: UnboundedReceiver<Submit>) {
    loop {
        match session(&cfg, &shared, &mut rx).await {
            Ok(()) => eprintln!("[stratum] connection closed"),
            Err(e) => eprintln!("[stratum] error: {e:#}"),
        }
        shared.set_job(None);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn session(cfg: &Config, shared: &Shared, rx: &mut UnboundedReceiver<Submit>) -> Result<()> {
    println!("[stratum] connecting to {}", cfg.pool);
    let stream = TcpStream::connect(&cfg.pool).await?;
    stream.set_nodelay(true)?;
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();

    send(&mut w, json!({"id":1,"method":"mining.subscribe","params":["bitcoin-lottery/0.1"]})).await?;
    let mut en1: Vec<u8> = Vec::new();
    let mut en2_size = 4usize;
    let user = format!("{}.{}", cfg.address, cfg.worker);
    let mut next_id = 100u64;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { return Ok(()) };
                if line.trim().is_empty() { continue; }
                let v: Value = serde_json::from_str(&line)?;
                if let Some(method) = v["method"].as_str() {
                    let p = v["params"].as_array().cloned().unwrap_or_default();
                    match method {
                        "mining.set_difficulty" => {
                            if let Some(d) = p.first().and_then(|d| d.as_f64()) {
                                shared.set_share_diff(d);
                                println!("[stratum] share difficulty = {d}");
                            }
                        }
                        "mining.notify" => match Job::from_notify(&p, en1.clone(), en2_size) {
                            Ok(job) => {
                                println!("[stratum] new job {}", job.job_id);
                                shared.set_job(Some(Arc::new(job)));
                            }
                            Err(e) => eprintln!("[stratum] bad notify: {e:#}"),
                        },
                        _ => {}
                    }
                    continue;
                }
                match v["id"].as_u64() {
                    Some(1) => {
                        let res = &v["result"];
                        en1 = hex::decode(res[1].as_str().unwrap_or(""))?;
                        en2_size = res[2].as_u64().unwrap_or(4) as usize;
                        send(&mut w, json!({"id":2,"method":"mining.authorize","params":[user, "x"]})).await?;
                    }
                    Some(2) => {
                        if v["result"].as_bool() != Some(true) {
                            bail!("authorization failed: {}", v["error"]);
                        }
                        println!("[stratum] authorized as {user}");
                        if cfg.suggest_difficulty > 0.0 {
                            send(&mut w, json!({"id":3,"method":"mining.suggest_difficulty",
                                "params":[cfg.suggest_difficulty]})).await?;
                        }
                    }
                    Some(id) if id >= 100 => {
                        if v["result"].as_bool() == Some(true) {
                            if shared.accepted.fetch_add(1, Relaxed) == 0 {
                                let _ = shared.notify.send("First share accepted by the pool. Mining works.".into());
                            }
                            println!("[share] accepted");
                        } else {
                            shared.rejected.fetch_add(1, Relaxed);
                            println!("[share] rejected: {}", v["error"]);
                        }
                    }
                    _ => {}
                }
            }
            sub = rx.recv() => {
                let Some(s) = sub else { return Ok(()) };
                next_id += 1;
                send(&mut w, json!({"id":next_id,"method":"mining.submit",
                    "params":[user, s.job_id, s.extranonce2, s.ntime, s.nonce]})).await?;
            }
        }
    }
}

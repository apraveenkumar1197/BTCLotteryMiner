mod hasher;
mod job;
mod notify;
mod stratum;

use clap::Parser;
use hasher::{hash_difficulty, nbits_difficulty, Hasher};
use job::{en2_bytes, Job};
use std::sync::atomic::{AtomicU64, Ordering::*};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// Solo Bitcoin lottery miner (Stratum v1, CPU).
#[derive(Parser)]
pub struct Config {
    /// Pool host:port (solo pool)
    #[arg(long, env = "POOL", default_value = "stratum.ckpool.org:3333")]
    pub pool: String,
    /// Your BTC address (block reward goes here if you win)
    #[arg(long, env = "BTC_ADDRESS")]
    pub address: String,
    /// Worker name
    #[arg(long, env = "WORKER", default_value = "lotto")]
    pub worker: String,
    /// Hashing threads (default: all logical cores)
    #[arg(long, env = "THREADS")]
    pub threads: Option<usize>,
    /// Suggested share difficulty (pool minimum is 10000 unless lowered; 1 = fastest feedback,
    /// no effect on block odds). 0 disables the suggestion.
    #[arg(long, env = "SUGGEST_DIFFICULTY", default_value_t = 1.0)]
    pub suggest_difficulty: f64,
    /// Telegram bot token from @BotFather (optional; keep secret)
    #[arg(long, env = "TELEGRAM_BOT_TOKEN", hide_env_values = true)]
    pub telegram_bot_token: Option<String>,
    /// Telegram chat id to notify (optional)
    #[arg(long, env = "TELEGRAM_CHAT_ID")]
    pub telegram_chat_id: Option<String>,
    /// Seconds between stats lines
    #[arg(long, env = "STATS_INTERVAL", default_value_t = 5)]
    pub stats_interval: u64,
}

pub struct Submit {
    pub job_id: String,
    pub extranonce2: String,
    pub ntime: String,
    pub nonce: String,
}

pub struct Shared {
    job: RwLock<Option<Arc<Job>>>,
    pub generation: AtomicU64,
    share_diff: AtomicU64, // f64 bits
    best_diff: AtomicU64,  // f64 bits
    pub hashes: AtomicU64,
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    pub notify: UnboundedSender<String>,
}

impl Shared {
    fn new(notify: UnboundedSender<String>) -> Self {
        Shared {
            notify,
            job: RwLock::new(None),
            generation: AtomicU64::new(0),
            share_diff: AtomicU64::new(1f64.to_bits()),
            best_diff: AtomicU64::new(0f64.to_bits()),
            hashes: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// The job is stored before the generation bump (workers read generation first).
    pub fn set_job(&self, j: Option<Arc<Job>>) {
        *self.job.write().unwrap() = j;
        self.generation.fetch_add(1, Release);
    }

    pub fn set_share_diff(&self, d: f64) {
        self.share_diff.store(d.to_bits(), Relaxed);
    }

    fn share_diff(&self) -> f64 {
        f64::from_bits(self.share_diff.load(Relaxed))
    }

    fn note_best(&self, d: f64) {
        let _ = self.best_diff.fetch_update(Relaxed, Relaxed, |cur| {
            (d > f64::from_bits(cur)).then(|| d.to_bits())
        });
    }
}

fn worker(tid: u64, nthreads: u64, shared: Arc<Shared>, tx: UnboundedSender<Submit>) {
    loop {
        let gen = shared.generation.load(Acquire);
        let Some(job) = shared.job.read().unwrap().clone() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let net_diff = nbits_difficulty(job.nbits);
        let mut n = 0u64;
        'en2: loop {
            let en2 = en2_bytes(tid + n * nthreads, job.extranonce2_size);
            n += 1;
            let mut h = Hasher::new(&job.header_prefix(&en2));
            let mut share_diff = shared.share_diff();
            for nonce in 0..=u32::MAX {
                let o = h.hash(nonce);
                // diff >= 1 requires the top 32 bits of the hash to be zero.
                if o[7] == 0 || share_diff < 1.0 {
                    let d = hash_difficulty(&o);
                    shared.note_best(d);
                    if d >= share_diff {
                        if d >= net_diff {
                            println!("\n*** BLOCK FOUND!!! difficulty {d:.0} ***\n");
                            let _ = shared.notify.send(format!(
                                "BLOCK FOUND! Difficulty {d:.0}, submitted to the pool. Check your wallet and the pool stats."
                            ));
                        }
                        let _ = tx.send(Submit {
                            job_id: job.job_id.clone(),
                            extranonce2: hex::encode(&en2),
                            ntime: format!("{:08x}", job.ntime),
                            nonce: format!("{nonce:08x}"),
                        });
                    }
                }
                if nonce & 0xFFFF == 0xFFFF {
                    shared.hashes.fetch_add(0x10000, Relaxed);
                    if shared.generation.load(Acquire) != gen {
                        break 'en2;
                    }
                    share_diff = shared.share_diff();
                }
            }
        }
    }
}

fn fmt_rate(h: f64) -> String {
    match h {
        h if h >= 1e9 => format!("{:.2} GH/s", h / 1e9),
        h if h >= 1e6 => format!("{:.2} MH/s", h / 1e6),
        h if h >= 1e3 => format!("{:.2} KH/s", h / 1e3),
        h => format!("{h:.0} H/s"),
    }
}

#[tokio::main]
async fn main() {
    let cfg = Arc::new(Config::parse());
    let (ntx, nrx) = unbounded_channel::<String>();
    let shared = Arc::new(Shared::new(ntx.clone()));
    let threads = cfg
        .threads
        .filter(|&t| t > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let (tx, rx) = unbounded_channel();

    println!("bitcoin-lottery: {threads} threads -> {} as {}.{}", cfg.pool, cfg.address, cfg.worker);
    println!("Reminder: odds of a block are astronomically small. Enjoy the ticket.\n");

    for t in 0..threads {
        let (s, tx) = (shared.clone(), tx.clone());
        std::thread::spawn(move || worker(t as u64, threads as u64, s, tx));
    }
    tokio::spawn(stratum::run(cfg.clone(), shared.clone(), rx));
    tokio::spawn(notify::run(cfg.clone(), nrx));
    let _ = ntx.send(format!(
        "Miner started: {threads} threads, pool {}, worker {}.{}",
        cfg.pool, cfg.address, cfg.worker
    ));

    let start = Instant::now();
    let (mut last_h, mut last_t) = (0u64, Instant::now());
    loop {
        tokio::time::sleep(Duration::from_secs(cfg.stats_interval.max(1))).await;
        let h = shared.hashes.load(Relaxed);
        let rate = (h - last_h) as f64 / last_t.elapsed().as_secs_f64();
        (last_h, last_t) = (h, Instant::now());
        println!(
            "[{:>5}s] {:>12} | shares ok/rej {}/{} | best diff {:.0} | pool diff {}",
            start.elapsed().as_secs(),
            fmt_rate(rate),
            shared.accepted.load(Relaxed),
            shared.rejected.load(Relaxed),
            f64::from_bits(shared.best_diff.load(Relaxed)),
            shared.share_diff()
        );
    }
}

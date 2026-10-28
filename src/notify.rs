//! Optional Telegram notifications. Disabled unless both token and chat id are set.

use crate::Config;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn run(cfg: Arc<Config>, mut rx: UnboundedReceiver<String>) {
    let token = cfg.telegram_bot_token.clone().filter(|s| !s.trim().is_empty());
    let chat = cfg.telegram_chat_id.clone().filter(|s| !s.trim().is_empty());
    let (Some(token), Some(chat)) = (token, chat) else {
        // Keep draining so senders never fail.
        while rx.recv().await.is_some() {}
        return;
    };
    println!("[telegram] notifications enabled");
    let client = reqwest::Client::new();
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    while let Some(msg) = rx.recv().await {
        let res = client
            .post(&url)
            .form(&[("chat_id", chat.as_str()), ("text", msg.as_str())])
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => eprintln!("[telegram] HTTP {}", r.status()),
            // without_url() keeps the bot token out of the log.
            Err(e) => eprintln!("[telegram] error: {}", e.without_url()),
        }
    }
}

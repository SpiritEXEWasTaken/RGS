//! Posts to Discord through a channel webhook (Server Settings > Integrations > Webhooks).
//! Nothing here can ping anyone: mentions are switched off on every message.

use serde_json::{json, Value};

/// One new report, as a Discord embed. Sent in the background; a failure is only logged.
pub fn report(webhook: &str, id: i64, reason: &str, target: &str, target_account: Option<&str>, reporter: &str, game: &str, context: &str, details: &str) {
    let mut fields = vec![
        field("Reported", &match target_account {
            Some(account) if account != target => format!("{target} (account: {account})"),
            Some(_) => target.to_owned(),
            None => format!("{target} (not signed in)"),
        }),
        field("Reason", reason),
        field("Reported by", reporter),
        field("Game", game),
    ];
    if !context.is_empty() {
        fields.push(field("Where", context));
    }
    if !details.is_empty() {
        fields.push(json!({ "name": "Details", "value": clip(details, 1000), "inline": false }));
    }
    let body = json!({
        "username": "RGS Reports",
        "allowed_mentions": { "parse": [] },
        "embeds": [{
            "title": format!("New report #{id}: {reason}"),
            "color": 0xF0B24A,
            "fields": fields,
            "footer": { "text": "Handle it in the RGS admin panel" },
        }],
    });
    send(webhook.to_owned(), body);
}

fn field(name: &str, value: &str) -> Value {
    json!({ "name": name, "value": clip(value, 200), "inline": true })
}

fn clip(s: &str, max: usize) -> String {
    let s = if s.trim().is_empty() { "-" } else { s.trim() };
    s.chars().take(max).collect()
}

fn send(webhook: String, body: Value) {
    tokio::spawn(async move {
        let result = reqwest::Client::new().post(&webhook).json(&body).send().await;
        match result {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => tracing::warn!("Discord webhook answered {}", r.status()),
            Err(e) => tracing::warn!("couldn't reach the Discord webhook: {e}"),
        }
    });
}

//! Provider-reported token accounting. Input totals include cached tokens once.
use super::Config;
use crate::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_COUNT: u64 = 9_007_199_254_740_991;
const FIELDS: [&str; 7] = [
    "inputTokens",
    "outputTokens",
    "uncachedInputTokens",
    "cacheReadTokens",
    "cacheCreationTokens",
    "reasoningTokens",
    "streamMs",
];

pub(super) fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|n| *n <= MAX_COUNT)
}

pub(super) fn valid(value: &Value) -> bool {
    value.is_null()
        || value.as_object().is_some_and(|fields| {
            fields.iter().all(|(name, value)| {
                (FIELDS.contains(&name.as_str()) || name == "latestContextTokens")
                    && count(value).is_some()
            })
        })
}

/// Derived display data; kept out of the persisted provider accounting fields.
#[allow(
    clippy::cast_precision_loss,
    reason = "Token counts are bounded by MAX_COUNT and converted only for display ratios."
)]
pub(super) fn details(value: &Value) -> Value {
    let input = count(&value["inputTokens"]);
    let output = count(&value["outputTokens"]);
    let read = count(&value["cacheReadTokens"]);
    let ratio = input
        .zip(read)
        .filter(|(input, read)| *input > 0 && read <= input)
        .map(|(input, read)| read as f64 / input as f64);
    let total = input
        .zip(output)
        .and_then(|(input, output)| input.checked_add(output))
        .filter(|total| *total <= MAX_COUNT);
    let speed = output
        .zip(count(&value["streamMs"]))
        .filter(|(_, ms)| *ms > 0)
        .map(|(output, ms)| output as f64 * 1000.0 / ms as f64);
    json!({"totalTokens":total,"cacheStatus":match ratio {
        Some(value) if value > 0.0 => "hit", Some(_) => "miss", None => "unknown",
    },"cacheHitRate":ratio.map(|value| value * 100.0),"outputTokensPerSecond":speed})
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn derived_details_include_subsets_once_and_leave_missing_counts_unknown() {
        let reported = details(
            &json!({"inputTokens":100,"outputTokens":20,"cacheReadTokens":40,"cacheCreationTokens":10,"reasoningTokens":5,"streamMs":2000}),
        );
        assert_eq!(
            reported,
            json!({"totalTokens":120,"cacheStatus":"hit","cacheHitRate":40.0,"outputTokensPerSecond":10.0})
        );
        assert_eq!(
            details(&json!({"inputTokens":100,"cacheReadTokens":0}))["cacheStatus"],
            "miss"
        );
        for usage in [
            Value::Null,
            json!({}),
            json!({"inputTokens":100}),
            json!({"outputTokens":20,"streamMs":0}),
            json!({"inputTokens":100,"cacheReadTokens":101}),
            json!({"inputTokens":0,"cacheReadTokens":0}),
        ] {
            let derived = details(&usage);
            assert!(derived["totalTokens"].is_null());
            assert_eq!(derived["cacheStatus"], "unknown");
            assert!(derived["cacheHitRate"].is_null());
            assert!(derived["outputTokensPerSecond"].is_null());
        }
        assert!(
            details(&json!({"inputTokens":MAX_COUNT,"outputTokens":1}))["totalTokens"].is_null()
        );
    }
}

#[derive(Default)]
pub(super) struct Counter {
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
    reasoning: Option<u64>,
}

impl Counter {
    pub(super) fn update(&mut self, kind: &str, value: &Value) {
        let input = ["input_tokens", "prompt_tokens", "promptTokenCount"]
            .iter()
            .find_map(|key| count(&value[*key]));
        let output = ["output_tokens", "completion_tokens", "candidatesTokenCount"]
            .iter()
            .find_map(|key| count(&value[*key]));
        let cache_fields = [
            count(&value["input_tokens_details"]["cached_tokens"]),
            count(&value["prompt_tokens_details"]["cached_tokens"]),
            count(&value["prompt_cache_hit_tokens"]),
            count(&value["cache_read_input_tokens"]),
            count(&value["cachedContentTokenCount"]),
        ];
        // Some compatible APIs send a zero native field alongside their
        // populated vendor field. Prefer a measured nonzero cache count.
        let cached = cache_fields
            .iter()
            .flatten()
            .copied()
            .find(|n| *n > 0)
            .or_else(|| cache_fields.iter().flatten().copied().next());
        let created = count(&value["cache_creation_input_tokens"])
            .or_else(|| count(&value["input_tokens_details"]["cache_write_tokens"]));
        let reasoning = count(&value["output_tokens_details"]["reasoning_tokens"])
            .or_else(|| count(&value["completion_tokens_details"]["reasoning_tokens"]))
            .or_else(|| count(&value["thoughtsTokenCount"]));
        // SSE usage reports are cumulative snapshots, including Anthropic's
        // separate message_start and message_delta events, rather than deltas.
        self.input = input.or(self.input);
        self.output = output.or(self.output);
        self.cache_read = cached.or(self.cache_read);
        self.cache_write = created.or(self.cache_write);
        self.reasoning = reasoning.or(self.reasoning);
        if !matches!(kind, "anthropic" | "responses") {
            self.cache_write = None;
        }
    }

    pub(super) fn value(&self, kind: &str) -> Value {
        let mut value = json!({});
        let read = self.cache_read.unwrap_or(0);
        let write = self.cache_write.unwrap_or(0);
        if let Some(input) = self.input {
            let total = if kind == "anthropic" {
                input.checked_add(read).and_then(|n| n.checked_add(write))
            } else {
                Some(input)
            };
            if let Some(total) = total.filter(|n| *n <= MAX_COUNT) {
                value["inputTokens"] = json!(total);
                value["latestContextTokens"] = json!(total);
                // Ignore impossible cache subsets without inflating the prompt.
                if read <= total && write <= total - read {
                    if self.cache_read.is_some() {
                        value["uncachedInputTokens"] = json!(total - read - write);
                        value["cacheReadTokens"] = json!(read);
                    }
                    if self.cache_write.is_some() {
                        value["cacheCreationTokens"] = json!(write);
                    }
                }
            }
        }
        if let Some(output) = self.output {
            // Gemini's candidate count excludes thinking; OpenAI's includes it.
            let output = if kind == "gemini" {
                output.checked_add(self.reasoning.unwrap_or(0))
            } else {
                Some(output)
            };
            if let Some(output) = output.filter(|n| *n <= MAX_COUNT) {
                value["outputTokens"] = json!(output);
            }
        }
        if let Some(reasoning) = self.reasoning {
            value["reasoningTokens"] = json!(reasoning);
        }
        if value.as_object().is_none_or(serde_json::Map::is_empty) {
            Value::Null
        } else {
            value
        }
    }
}

pub(super) struct Turn {
    pub chat_id: String,
    pub provider_id: String,
    pub provider_label: String,
    pub model: String,
    pub model_name: String,
    pub estimated: bool,
    pub completed_at: u64,
    pub usage: Value,
}

#[derive(Default)]
struct Stats {
    counts: BTreeMap<String, u64>,
    replies: u64,
    unreported: u64,
    days: BTreeSet<u64>,
    chats: BTreeSet<String>,
    missing_dates: bool,
    legacy: bool,
    stream_ms: u64,
    stream_output: u64,
    cache_input: u64,
    cache_read: u64,
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .filter(|n| *n <= MAX_COUNT)
        .ok_or("Token usage totals exceed the supported range.".into())
}

impl Stats {
    fn record(&mut self, turn: &Turn) -> Result<()> {
        if count(&turn.usage["inputTokens"]).is_none()
            && count(&turn.usage["outputTokens"]).is_none()
        {
            self.unreported += 1;
            return Ok(());
        }
        self.replies += 1;
        self.chats.insert(turn.chat_id.clone());
        self.missing_dates |= turn.completed_at == 0;
        if turn.completed_at > 0 {
            self.days.insert(turn.completed_at / 86_400);
        }
        self.legacy |= turn.usage.get("uncachedInputTokens").is_none();
        for name in FIELDS {
            if let Some(n) = count(&turn.usage[name]) {
                let sum = add(self.counts.get(name).copied().unwrap_or(0), n)?;
                self.counts.insert(name.into(), sum);
            }
        }
        if let (Some(input), Some(read)) = (
            count(&turn.usage["inputTokens"]).filter(|n| *n > 0),
            count(&turn.usage["cacheReadTokens"]),
        ) && read <= input
        {
            self.cache_input = add(self.cache_input, input)?;
            self.cache_read = add(self.cache_read, read)?;
        }
        if let (Some(ms), Some(output)) = (
            count(&turn.usage["streamMs"]).filter(|n| *n > 0),
            count(&turn.usage["outputTokens"]).filter(|n| *n > 0),
        ) {
            // Only paired duration/output samples contribute to the rate.
            self.stream_ms = add(self.stream_ms, ms)?;
            self.stream_output = add(self.stream_output, output)?;
        }
        Ok(())
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "Aggregated usage ratios and averages are approximate display values."
    )]
    fn value(&self) -> Value {
        let mut value = json!({});
        for field in FIELDS {
            value[field] = json!(self.counts.get(field));
        }
        let input = self.counts.get("inputTokens").copied().unwrap_or(0);
        let output = self.counts.get("outputTokens").copied().unwrap_or(0);
        value["cacheHitRate"] = json!(if self.cache_input > 0 {
            Some(self.cache_read as f64 / self.cache_input as f64 * 100.0)
        } else {
            None
        });
        value["outputTokensPerSecond"] = json!(if self.stream_ms > 0 {
            Some(self.stream_output as f64 * 1000.0 / self.stream_ms as f64)
        } else {
            None
        });
        value["replyCount"] = json!(self.replies);
        value["unreportedReplies"] = json!(self.unreported);
        value["chatCount"] = json!(self.chats.len());
        value["activeDays"] = json!((!self.missing_dates).then_some(self.days.len()));
        value["averagePerChat"] = json!(
            (!self.chats.is_empty())
                .then(|| (input as f64 + output as f64) / self.chats.len() as f64)
        );
        value["averagePerDay"] = json!(
            (!self.missing_dates && !self.days.is_empty())
                .then(|| (input as f64 + output as f64) / self.days.len() as f64)
        );
        value["legacyUsage"] = json!(self.legacy);
        value
    }
}

pub(super) fn report(turns: &[Turn], cfg: &Config) -> Result<Value> {
    let mut total = Stats::default();
    let mut groups: BTreeMap<(String, String, bool), (&Turn, Stats)> = BTreeMap::new();
    for turn in turns {
        total.record(turn)?;
        if !turn.usage.is_object() {
            continue;
        }
        let provider = if turn.provider_id.is_empty() {
            format!("legacy:{}", turn.provider_label)
        } else {
            turn.provider_id.clone()
        };
        let (_, stats) = groups
            .entry((provider, turn.model.clone(), turn.estimated))
            .or_insert_with(|| (turn, Stats::default()));
        stats.record(turn)?;
    }
    let mut models = groups
        .into_iter()
        .filter(|(_, (_, stats))| stats.replies > 0)
        .map(|((provider, model, estimated), (turn, stats))| {
            let configured = cfg.providers.iter().any(|p| {
                (p.id == turn.provider_id
                    || (turn.provider_id.is_empty() && p.label == turn.provider_label))
                    && p.models.iter().any(|m| m.id == model)
            });
            json!({"key":format!("{provider}:{model}:{estimated}"),
                "providerId":provider,"providerLabel":turn.provider_label,
                "model":model,"modelName":turn.model_name,
                "estimated":estimated,"removed":!configured,"stats":stats.value()})
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        b["stats"]["inputTokens"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["stats"]["inputTokens"].as_u64().unwrap_or(0))
            .then_with(|| a["key"].as_str().cmp(&b["key"].as_str()))
    });
    let mut providers: BTreeMap<String, Value> = BTreeMap::new();
    for model in models {
        let id = model["providerId"].as_str().unwrap_or_default().to_owned();
        let group = providers.entry(id.clone()).or_insert_with(|| {
            json!({
                "id":id,"label":model["providerLabel"],"models":[]
            })
        });
        group["models"]
            .as_array_mut()
            .ok_or("Invalid usage model group.")?
            .push(model);
    }
    Ok(json!({"stats":total.value(),"providers":providers.into_values().collect::<Vec<_>>()}))
}

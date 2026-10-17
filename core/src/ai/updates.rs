//! Bounded, revisioned projections for the chat UI. Private provider state never crosses this path.
use super::*;

const FIELDS: &[&str] = &[
    "status",
    "text",
    "reasoning",
    "blocks",
    "games",
    "error",
    "usage",
    "reasoningEffort",
    "model",
    "providerLabel",
];

#[derive(Default)]
pub(super) struct Cursor {
    state: Value,
    revision: u64,
    delta: Value,
}

fn projection(state: &Value) -> Value {
    let mut value = json!({});
    for field in FIELDS {
        value[*field] = state[*field].clone();
    }
    value["usageDetails"] = usage::details(&state["usage"]);
    value
}

fn array_delta(previous: &Value, next: &Value) -> Value {
    let old = previous.as_array().map(Vec::as_slice).unwrap_or_default();
    let new = next.as_array().map(Vec::as_slice).unwrap_or_default();
    let upserts: Vec<_> = new
        .iter()
        .enumerate()
        .filter(|(index, item)| old.get(*index) != Some(*item))
        .map(|(index, item)| json!({"index":index,"value":item}))
        .collect();
    json!({"length":new.len(),"upserts":upserts})
}

impl Cursor {
    pub fn poll(&mut self, state: &Value, after: Option<u64>) -> Value {
        if FIELDS
            .iter()
            .any(|field| self.state[*field] != state[*field])
        {
            let next = projection(state);
            let base = self.revision;
            self.revision += 1;
            let mut fields = json!({});
            let mut strings = json!({});
            let mut arrays = json!({});
            for (key, value) in next.as_object().unwrap() {
                let old = &self.state[key];
                if old == value {
                    continue;
                }
                match key.as_str() {
                    "text" | "reasoning" => {
                        strings[key] = match (old.as_str(), value.as_str()) {
                            (Some(old), Some(new)) if new.starts_with(old) => {
                                json!({"append":&new[old.len()..]})
                            }
                            _ => json!({"replace":value.as_str().unwrap_or_default()}),
                        };
                    }
                    "blocks" | "games" => arrays[key] = array_delta(old, value),
                    _ => fields[key] = value.clone(),
                }
            }
            self.delta = json!({"kind":"delta","baseRevision":base,"revision":self.revision,
                "fields":fields,"strings":strings,"arrays":arrays});
            self.state = next;
        }
        if after == Some(self.revision) {
            json!({"kind":"unchanged","revision":self.revision})
        } else if after.is_some() && after == self.delta["baseRevision"].as_u64() {
            self.delta.clone()
        } else {
            json!({"kind":"snapshot","revision":self.revision,"state":self.state})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_project_only_public_changes_and_recover_gaps() {
        let mut cursor = Cursor::default();
        let mut state = json!({"status":"running","text":"Hola 🌎","reasoning":"", "blocks":[{"id":"one","content":"Hola"}],"games":[],"usage":{"inputTokens":10,"outputTokens":2},"privateToken":"secret","result":{"private":"secret"}});
        let first = cursor.poll(&state, None);
        assert_eq!(first["kind"], "snapshot");
        assert_eq!(first["revision"], 1);
        assert_eq!(first["state"]["usageDetails"]["totalTokens"], 12);
        assert!(first["state"].get("privateToken").is_none());
        assert!(first["state"].get("result").is_none());
        state["privateToken"] = json!("changed");
        assert_eq!(
            cursor.poll(&state, Some(1)),
            json!({"kind":"unchanged","revision":1})
        );
        state["text"] = json!("Hola 🌎. Buen día");
        state["blocks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"two","content":"Buen día"}));
        let delta = cursor.poll(&state, Some(1));
        assert_eq!(delta["strings"]["text"], json!({"append":". Buen día"}));
        assert_eq!(
            delta["arrays"]["blocks"],
            json!({"length":2,"upserts":[{"index":1,"value":{"id":"two","content":"Buen día"}}]})
        );
        assert_eq!(
            cursor.poll(&state, Some(1)),
            delta,
            "one delta can be retried"
        );
        state["status"] = json!("completed");
        state["text"] = json!("Replaced");
        state["blocks"] = json!([]);
        let final_delta = cursor.poll(&state, Some(2));
        assert_eq!(final_delta["fields"]["status"], "completed");
        assert_eq!(
            final_delta["strings"]["text"],
            json!({"replace":"Replaced"})
        );
        assert_eq!(
            final_delta["arrays"]["blocks"],
            json!({"length":0,"upserts":[]})
        );
        for after in [None, Some(1), Some(100)] {
            let recovered = cursor.poll(&state, after);
            assert_eq!(recovered["kind"], "snapshot");
            assert_eq!(recovered["revision"], 3);
            assert_eq!(recovered["state"]["text"], "Replaced");
        }
    }
}

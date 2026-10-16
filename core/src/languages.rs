use super::{clean, folded, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, io::ErrorKind, path::Path};

const MAX_LANGUAGES: usize = 50;
const MAX_STORAGE_BYTES: u64 = 64_000;

#[derive(Deserialize, Serialize, PartialEq)]
enum Level {
    A1,
    A2,
    B1,
    B2,
    C1,
    C2,
    #[serde(rename = "native")]
    Native,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Language {
    language: String,
    level: Level,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LanguageSettings {
    known: Vec<Language>,
    learning: Vec<Language>,
}

impl LanguageSettings {
    fn normalize(&mut self) -> Result<()> {
        for (languages, learning) in [(&mut self.known, false), (&mut self.learning, true)] {
            if languages.len() > MAX_LANGUAGES {
                return Err("Keep each language list to 50 languages or fewer.".into());
            }
            let mut names = HashSet::new();
            for entry in languages {
                entry.language = clean(&entry.language);
                if entry.language.is_empty()
                    || entry.language.chars().count() > 80
                    || entry.language.chars().any(char::is_control)
                {
                    return Err("Enter a language name between 1 and 80 characters.".into());
                }
                if !names.insert(folded(&entry.language)) {
                    return Err(
                        "This language is already in this list. Edit its level instead.".into(),
                    );
                }
                if learning && entry.level == Level::Native {
                    return Err(
                        "Choose an A1–C2 target level for a language you want to learn.".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

pub(super) fn settings(command: &Value, root: &str) -> Result<Value> {
    let path = Path::new(root).join("settings").join("languages.json");
    let mut settings: LanguageSettings = if command["settings"].is_null() {
        match fs::metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => LanguageSettings::default(),
            Ok(metadata) if metadata.len() <= MAX_STORAGE_BYTES => {
                let bytes =
                    fs::read(&path).map_err(|_| "Could not read your language settings.")?;
                serde_json::from_slice(&bytes)
                    .map_err(|_| "Could not read your saved language settings.")?
            }
            _ => return Err("Could not read your saved language settings.".into()),
        }
    } else {
        serde_json::from_value(command["settings"].clone())
            .map_err(|_| "Choose a language and a valid level for each entry.")?
    };
    settings.normalize()?;
    let value =
        serde_json::to_value(settings).map_err(|_| "Could not encode language settings.")?;
    if !command["settings"].is_null() {
        fs::create_dir_all(path.parent().ok_or("Invalid settings path.")?)
            .map_err(|_| "Could not create language settings storage.")?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, value.to_string())
            .and_then(|_| fs::rename(&temporary, &path))
            .map_err(|_| "Could not save your languages. Please try again.")?;
    }
    Ok(value)
}

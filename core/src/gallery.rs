//! Bounded image-gallery planning and cache validation, following LiTube.
use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
const LIMIT: u64 = 20 * 1024 * 1024;
pub(crate) fn allowed(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| "Invalid image address.")?;
    if raw.len() > 8192
        || raw.contains(['\r', '\n'])
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !url.host_str().is_some_and(|host| {
            ["ggpht.com", "ytimg.com", "googleusercontent.com"]
                .iter()
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        })
    {
        return Err("Unsupported image address.".into());
    }
    Ok(url)
}
fn directory(root: &str) -> PathBuf {
    Path::new(root).join("media").join("gallery")
}
fn key(url: &str) -> String {
    format!("{:x}", Sha256::digest(url.as_bytes()))
}
fn image(path: &Path) -> Result<Value> {
    let meta = fs::metadata(path).map_err(|_| "Image file is missing.")?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > LIMIT {
        return Err("Image exceeds the 20 MB limit.".into());
    }
    let mut header = [0u8; 64];
    let count = fs::File::open(path)
        .and_then(|mut f| f.read(&mut header))
        .map_err(|_| "Could not read image.")?;
    let bytes = &header[..count];
    let (extension, mime) = if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ("jpg", "image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", "image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ("gif", "image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        ("webp", "image/webp")
    } else if bytes.get(4..8) == Some(b"ftyp")
        && bytes.windows(4).any(|b| b == b"avif" || b == b"avis")
    {
        ("avif", "image/avif")
    } else if bytes.get(4..8) == Some(b"ftyp")
        && bytes
            .windows(4)
            .any(|b| b == b"heic" || b == b"heix" || b == b"heif")
    {
        ("heic", "image/heic")
    } else {
        return Err("Unsupported image format.".into());
    };
    Ok(
        json!({"uri":Url::from_file_path(path).map_err(|_|"Invalid image storage.")?.to_string(),"bytes":meta.len(),"extension":extension,"mimeType":mime,"name":format!("GoVue-{}.{}",SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),extension)}),
    )
}
fn prune(root: &str, protect: Option<&Path>) {
    let Ok(entries) = fs::read_dir(directory(root)) else {
        return;
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let time = meta.modified().ok()?;
            Some((path, meta.len(), time))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|f| f.2);
    let mut total = files.iter().map(|f| f.1).sum::<u64>();
    for (path, size, time) in files {
        if protect == Some(path.as_path()) {
            continue;
        }
        let old = time.elapsed().is_ok_and(|age| age.as_secs() > 86400);
        if (old || total > 64 * 1024 * 1024 && path.extension().is_some_and(|e| e != "part"))
            && fs::remove_file(path).is_ok()
        {
            total = total.saturating_sub(size);
        }
    }
}
pub(crate) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    match crate::string(command, "op")? {
        "gallery_validate" => {
            let urls = command["urls"]
                .as_array()
                .filter(|u| !u.is_empty() && u.len() <= 30)
                .ok_or("Invalid image gallery.")?;
            let urls = urls
                .iter()
                .map(|u| {
                    allowed(u.as_str().ok_or("Invalid image address.")?).map(|u| u.to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            prune(root, None);
            Ok(
                json!({"urls":urls,"index":command["index"].as_u64().unwrap_or(0).min(urls.len().saturating_sub(1) as u64)}),
            )
        }
        "gallery_request" => {
            let url = allowed(crate::string(command, "url")?)?.to_string();
            let cache_key = key(&url);
            for ext in ["jpg", "png", "gif", "webp", "avif", "heic"] {
                let path = directory(root).join(format!("{cache_key}.{ext}"));
                if path.is_file()
                    && let Ok(cached) = image(&path)
                {
                    let _ =
                        fs::File::open(&path).and_then(|file| file.set_modified(SystemTime::now()));
                    return Ok(json!({"image":cached}));
                }
            }
            Ok(
                json!({"request":{"url":url,"sourceUrl":url,"key":cache_key,"redirect":0,"maxBytes":LIMIT}}),
            )
        }
        "gallery_response" => {
            let request = &command["request"];
            let original = allowed(crate::string(request, "sourceUrl")?)?.to_string();
            let url = allowed(crate::string(request, "url")?)?;
            let cache_key = key(&original);
            if request["key"] != cache_key {
                return Err("Image request identity changed.".into());
            }
            let status = command["response"]["status"].as_u64().unwrap_or(0);
            if (300..400).contains(&status) {
                let hop = request["redirect"].as_u64().unwrap_or(4);
                if hop >= 3 {
                    return Err("Too many image redirects.".into());
                }
                let location = crate::string(&command["response"], "location")?;
                let next = url.join(location).map_err(|_| "Invalid image redirect.")?;
                allowed(next.as_str())?;
                return Ok(
                    json!({"request":{"url":next.to_string(),"sourceUrl":original,"key":cache_key,"redirect":hop+1,"maxBytes":LIMIT}}),
                );
            }
            if !(200..300).contains(&status) {
                return Err("Image could not load. Try again.".into());
            }
            let temporary = directory(root).join(format!("{cache_key}.part"));
            let metadata = image(&temporary)?;
            let extension = crate::string(&metadata, "extension")?;
            let destination = directory(root).join(format!("{cache_key}.{extension}"));
            fs::rename(&temporary, &destination).map_err(|_| "Could not cache image.")?;
            let result = image(&destination)?;
            prune(root, Some(&destination));
            Ok(json!({"image":result}))
        }
        _ => Err(crate::clean("Unknown gallery action.").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_redirects_keep_the_original_identity_and_reject_untrusted_hosts() {
        let root = "unused-gallery-boundary-test";
        let original = "https://yt3.ggpht.com/public-image?token=memory-only";
        let request =
            dispatch(&json!({"op":"gallery_request","url":original}), root).unwrap()["request"]
                .clone();
        for location in [
            "http://yt3.ggpht.com/image",
            "https://ggpht.com.evil.org/image",
            "https://user:secret@yt3.ggpht.com/image",
            "https://yt3.ggpht.com:8443/image",
        ] {
            assert!(dispatch(&json!({"op":"gallery_response","request":request,"response":{"status":302,"location":location}}), root).is_err());
        }
        let next = dispatch(&json!({"op":"gallery_response","request":request,"response":{"status":302,"location":"https://i.ytimg.com/final.png"}}), root).unwrap()["request"].clone();
        assert_eq!(next["key"], request["key"]);
        assert_eq!(next["sourceUrl"], original);
        let mut exhausted = next.clone();
        exhausted["redirect"] = json!(3);
        assert!(dispatch(&json!({"op":"gallery_response","request":exhausted,"response":{"status":302,"location":"/another"}}), root).is_err());
        let mut forged = next;
        forged["key"] = json!("../outside");
        assert!(
            dispatch(
                &json!({"op":"gallery_response","request":forged,"response":{"status":200}}),
                root
            )
            .is_err()
        );
    }

    #[test]
    fn gallery_cache_publishes_only_bounded_images_and_keeps_urls_out_of_storage() {
        struct Store(PathBuf);
        impl Drop for Store {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let name = format!(
            "govue-gallery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let store = Store(std::env::temp_dir().join(name));
        let root = store.0.to_str().unwrap();
        let command =
            json!({"op":"gallery_request","url":"https://yt3.ggpht.com/image?token=memory-only"});
        let request = dispatch(&command, root).unwrap()["request"].clone();
        fs::create_dir_all(directory(root)).unwrap();
        let part = directory(root).join(format!("{}.part", request["key"].as_str().unwrap()));
        fs::write(&part, b"<html>not an image</html>").unwrap();
        assert!(
            dispatch(
                &json!({"op":"gallery_response","request":request,"response":{"status":200}}),
                root
            )
            .is_err()
        );
        // A complete 1×1 PNG; publishing checks its type before exposing a path.
        let png: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 4, 0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31,
            0, 3, 3, 2, 0, 238, 254, 95, 225, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        fs::write(&part, png).unwrap();
        fs::File::options()
            .write(true)
            .open(&part)
            .unwrap()
            .set_len(LIMIT + 1)
            .unwrap();
        assert!(
            dispatch(
                &json!({"op":"gallery_response","request":request,"response":{"status":200}}),
                root
            )
            .is_err()
        );
        fs::write(&part, png).unwrap();
        let published = dispatch(
            &json!({"op":"gallery_response","request":request,"response":{"status":200}}),
            root,
        )
        .unwrap();
        assert_eq!(published["image"]["mimeType"], "image/png");
        assert!(!part.exists());
        assert!(
            dispatch(&command, root).unwrap()["image"].is_object(),
            "the second request uses the validated cache"
        );
        let files = fs::read_dir(directory(root))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert!(!files[0].contains("token"));
        assert_eq!(files[0].len(), 68);
    }
}

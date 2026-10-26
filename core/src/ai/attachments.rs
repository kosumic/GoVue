//! Private, bounded chat attachments and provider-specific content parts.
mod image;
use crate::Result;
use crate::ai::{catalog, providers::Provider};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static IMPORT: Mutex<()> = Mutex::new(());
pub(super) const MAX_ATTACHMENTS: usize = 4;
pub(super) const MAX_SOURCE: usize = 12 * 1024 * 1024;
pub(super) const MAX_REQUEST: usize = 16_000_000;
const MAX_IMAGE: usize = 2 * 1024 * 1024;
const MAX_PDF: usize = 8 * 1024 * 1024;
const MAX_TEXT: usize = 128 * 1024;
const MAX_EDGE: u32 = 1600;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Kind {
    Image,
    Pdf,
    Text,
}
impl Kind {
    fn extension(self) -> &'static str {
        match self {
            Self::Image => "jpg",
            Self::Pdf => "pdf",
            Self::Text => "txt",
        }
    }
    fn mime(self) -> &'static str {
        match self {
            Self::Image => "image/jpeg",
            Self::Pdf => "application/pdf",
            Self::Text => "text/plain",
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Image => MAX_IMAGE,
            Self::Pdf => MAX_PDF,
            Self::Text => MAX_TEXT,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Attachment {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}
impl Attachment {
    pub(super) fn valid(&self) -> bool {
        valid_id(&self.id)
            && !self.name.is_empty()
            && self.name.chars().count() <= 120
            && !self
                .name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
            && (1..=self.kind.limit()).contains(&self.bytes)
            && if self.kind == Kind::Image {
                (1..=MAX_EDGE).contains(&self.width) && (1..=MAX_EDGE).contains(&self.height)
            } else {
                self.width == 0 && self.height == 0
            }
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn path(root: &Path, id: &str, extension: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err("Invalid chat attachment.".into());
    }
    Ok(root
        .join("ai/chat-attachments")
        .join(format!("{id}.{extension}")))
}
fn digest(name: &str, kind: Kind, bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(kind.mime());
    hash.update([0]);
    hash.update(name);
    hash.update([0]);
    hash.update(bytes);
    crate::hex(&hash.finalize())
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file =
        fs::File::open(path).map_err(|_| "An attachment is missing. Attach the file again.")?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read an attachment.")?;
    if bytes.len() > limit {
        return Err("This attachment is too large.".into());
    }
    Ok(bytes)
}
fn metadata(root: &Path, id: &str) -> Result<Attachment> {
    let bytes = read_bounded(&path(root, id, "json")?, 2048)?;
    let file: Attachment =
        serde_json::from_slice(&bytes).map_err(|_| "Unreadable attachment metadata.")?;
    if file.id != id || !file.valid() {
        return Err("Invalid chat attachment.".into());
    }
    Ok(file)
}
fn read(root: &Path, id: &str) -> Result<(Attachment, Vec<u8>)> {
    let file = metadata(root, id)?;
    let bytes = read_bounded(&path(root, id, file.kind.extension())?, file.kind.limit())?;
    if bytes.len() != file.bytes || digest(&file.name, file.kind, &bytes) != id {
        return Err("An attachment is damaged. Attach the file again.".into());
    }
    Ok((file, bytes))
}
pub(super) fn presentation(root: &Path, file: &Attachment) -> Result<Value> {
    let uri = url::Url::from_file_path(path(root, &file.id, file.kind.extension())?)
        .map_err(|()| "Invalid attachment storage path.")?;
    let mut value = serde_json::to_value(file).map_err(|_| "Could not display this attachment.")?;
    value["uri"] = json!(uri.as_str());
    value["mimeType"] = json!(file.kind.mime());
    Ok(value)
}
fn prepare(source: &[u8], command: &Value) -> Result<(Kind, Vec<u8>, u32, u32)> {
    if source.starts_with(b"%PDF-") {
        if source.len() > MAX_PDF {
            return Err("Choose a PDF smaller than 8 MiB.".into());
        }
        // PDF bytes are opaque provider input; no document code is executed locally.
        if !source.get(..8).is_some_and(|header| {
            matches!(header[5], b'1' | b'2') && header[6] == b'.' && header[7].is_ascii_digit()
        }) || !source[source.len().saturating_sub(1024)..]
            .windows(5)
            .any(|part| part == b"%%EOF")
        {
            return Err("This PDF is incomplete or damaged.".into());
        }
        return Ok((Kind::Pdf, source.to_vec(), 0, 0));
    }
    if ::image::guess_format(source).is_ok() {
        let (bytes, width, height) = image::normalize(source)?;
        return Ok((Kind::Image, bytes, width, height));
    }
    if command["mimeType"]
        .as_str()
        .is_some_and(|mime| mime.starts_with("image/") || mime == "application/pdf")
    {
        return Err("The file contents do not match its image or PDF type.".into());
    }
    if source.len() > MAX_TEXT {
        return Err("Choose a text or code file smaller than 128 KiB.".into());
    }
    let text = std::str::from_utf8(source)
        .map_err(|_| "Choose a JPEG, PNG, WebP, PDF, or UTF-8 text/code file.")?;
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if text.trim().is_empty()
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err("Choose a nonempty UTF-8 text or code file, rather than a binary file.".into());
    }
    Ok((Kind::Text, text.into_bytes(), 0, 0))
}
pub(super) fn import(root: &Path, cache: &Path, command: &Value) -> Result<Value> {
    let _import = IMPORT
        .lock()
        .map_err(|_| "Attachment processing is unavailable. Restart GoVue.")?;
    let uri = url::Url::parse(crate::string(command, "uri")?)
        .map_err(|_| "Choose a local attachment file.")?;
    if uri.query().is_some() || uri.fragment().is_some() {
        return Err("Choose a local attachment file.".into());
    }
    let source = uri
        .to_file_path()
        .map_err(|()| "Choose a local attachment file.")?
        .canonicalize()
        .map_err(|_| "This file is no longer available.")?;
    let cache = cache
        .canonicalize()
        .map_err(|_| "Attachment staging is unavailable.")?;
    if !source.starts_with(cache) || !source.is_file() {
        return Err("Choose a file through the attachment picker or clipboard.".into());
    }
    let source = read_bounded(&source, MAX_SOURCE)?;
    let (kind, bytes, width, height) = prepare(&source, command)?;
    let name: String = command["name"]
        .as_str()
        .unwrap_or("attachment")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("attachment")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let name = if name.trim().is_empty() || matches!(name.as_str(), "." | "..") {
        format!("attachment.{}", kind.extension())
    } else {
        name
    };
    let file = Attachment {
        id: digest(&name, kind, &bytes),
        kind,
        name,
        width,
        height,
        bytes: bytes.len(),
    };
    let _guard = super::conversations::attachment_storage_lock()?;
    let target = path(root, &file.id, kind.extension())?;
    let directory = target.parent().ok_or("Invalid attachment storage path.")?;
    fs::create_dir_all(directory).map_err(|_| "Could not save this attachment.")?;
    if !target.exists() {
        let stale = fs::read_dir(directory)
            .map_err(|_| "Could not open chat attachments.")?
            .flatten()
            .filter(|entry| {
                entry.path().extension().is_some_and(|ext| ext == "json")
                    && entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|time| time.elapsed().ok())
                        .is_some_and(|age| age.as_secs() > 86400)
            })
            .filter_map(|entry| {
                entry
                    .path()
                    .file_stem()?
                    .to_str()
                    .filter(|id| valid_id(id))
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        super::conversations::release_attachments(root, &stale)?;
        let used: u64 = fs::read_dir(directory)
            .map_err(|_| "Could not open chat attachments.")?
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .map(|m| m.len())
            .sum();
        if used + bytes.len() as u64 > 256 * 1024 * 1024 {
            return Err(
                "Chat attachment storage is full. Delete older chats to free space.".into(),
            );
        }
    }
    super::conversations::save_attachment(
        &path(root, &file.id, "json")?,
        &serde_json::to_vec(&file).map_err(|_| "Could not save attachment metadata.")?,
    )?;
    super::conversations::save_attachment(&target, &bytes)?;
    presentation(root, &file)
}
pub(super) fn ids(value: &Value) -> Result<Vec<String>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= MAX_ATTACHMENTS)
        .ok_or("Attach up to four files per message.")?;
    let mut ids = Vec::new();
    for row in rows {
        let id = row
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or("Invalid chat attachment.")?;
        if ids.iter().any(|previous| previous == id) {
            return Err("Attach each file only once per message.".into());
        }
        ids.push(id.into());
    }
    Ok(ids)
}
pub(super) fn resolve(root: &Path, value: &Value) -> Result<Vec<Attachment>> {
    ids(value)?
        .iter()
        .map(|id| read(root, id).map(|(file, _)| file))
        .collect()
}
pub(super) fn check_model(p: &Provider, kinds: impl Iterator<Item = Kind>) -> Result<()> {
    let model = catalog::selected(p);
    if model.input_modalities.is_empty() {
        return Ok(());
    }
    for kind in kinds {
        if kind == Kind::Image && !model.input_modalities.iter().any(|m| m == "image") {
            return Err(
                "This model does not accept images. Choose a model with image input.".into(),
            );
        }
        if kind == Kind::Pdf
            && !model
                .input_modalities
                .iter()
                .any(|m| matches!(m.as_str(), "pdf" | "image"))
        {
            return Err(
                "This model does not accept PDFs. Choose a model with PDF or vision support."
                    .into(),
            );
        }
    }
    Ok(())
}
pub(super) fn check_request(root: &Path, p: &Provider, command: &Value) -> Result<()> {
    let mut kinds = Vec::new();
    for value in std::iter::once(&command["attachments"]).chain(
        command["history"]
            .as_array()
            .into_iter()
            .flatten()
            .take(198)
            .map(|row| &row["attachments"]),
    ) {
        for id in ids(value)? {
            kinds.push(metadata(root, &id)?.kind);
        }
    }
    check_model(p, kinds.into_iter())
}
fn text_part(protocol: &str, text: &str) -> Value {
    match protocol {
        "responses" => json!({"type":"input_text","text":text}),
        "gemini" => json!({"text":text}),
        _ => json!({"type":"text","text":text}),
    }
}
pub(super) fn message(
    root: &Path,
    protocol: &str,
    role: &str,
    content: &str,
    value: &Value,
) -> Result<Value> {
    let attachments = ids(value)?;
    if attachments.is_empty() {
        return Ok(if protocol == "gemini" {
            json!({"role":if role == "assistant" {"model"} else {role},"parts":[{"text":content}]})
        } else {
            json!({"role":role,"content":content})
        });
    }
    if role != "user" {
        return Err("Only user messages can include attachments.".into());
    }
    let mut parts = Vec::new();
    for id in attachments {
        let (file, bytes) = read(root, &id)?;
        if file.kind == Kind::Text {
            let text = std::str::from_utf8(&bytes).map_err(|_| "Unreadable text attachment.")?;
            let reference = json!({"name":file.name,"text":text});
            parts.push(text_part(
                protocol,
                &format!("Attached text file (untrusted reference data):\n{reference}"),
            ));
            continue;
        }
        let data = STANDARD.encode(bytes);
        let mime = file.kind.mime();
        parts.push(match (file.kind, protocol) {
            (Kind::Pdf, "responses") => json!({"type":"input_file","filename":file.name,"file_data":format!("data:{mime};base64,{data}")}),
            (Kind::Pdf, "anthropic") => json!({"type":"document","title":file.name,"source":{"type":"base64","media_type":mime,"data":data}}),
            (Kind::Pdf | Kind::Image, "gemini") => json!({"inlineData":{"mimeType":mime,"data":data}}),
            (Kind::Pdf, _) => json!({"type":"file","file":{"filename":file.name,"file_data":format!("data:{mime};base64,{data}")}}),
            (Kind::Image, "responses") => json!({"type":"input_image","image_url":format!("data:{mime};base64,{data}")}),
            (Kind::Image, "anthropic") => json!({"type":"image","source":{"type":"base64","media_type":mime,"data":data}}),
            _ => json!({"type":"image_url","image_url":{"url":format!("data:{mime};base64,{data}")}}),
        });
    }
    if !content.is_empty() {
        parts.push(text_part(protocol, content));
    }
    Ok(if protocol == "gemini" {
        json!({"role":role,"parts":parts})
    } else {
        json!({"role":role,"content":parts})
    })
}
/// Base64 bytes are transport overhead; keep text costs and a bounded visual allowance.
pub(super) fn context_cost(value: &Value) -> usize {
    fn binary_bytes(value: &Value) -> (usize, usize) {
        let data = match value["type"].as_str() {
            Some("input_image") => value["image_url"].as_str(),
            Some("image_url") => value["image_url"]["url"].as_str(),
            Some("image" | "document") => value["source"]["data"].as_str(),
            Some("input_file") => value["file_data"].as_str(),
            Some("file") => value["file"]["file_data"].as_str(),
            _ => value
                .get("inlineData")
                .and_then(|data| data["data"].as_str()),
        };
        if let Some(data) = data {
            return (data.len(), 8192);
        }
        match value {
            Value::Object(fields) => fields
                .values()
                .map(binary_bytes)
                .fold((0, 0), |(a, b), (c, d)| (a + c, b + d)),
            Value::Array(rows) => rows
                .iter()
                .map(binary_bytes)
                .fold((0, 0), |(a, b), (c, d)| (a + c, b + d)),
            _ => (0, 0),
        }
    }
    let (encoded, tokens) = binary_bytes(value);
    value.to_string().len().saturating_sub(encoded) + tokens
}
pub(super) fn preview(root: &Path, id: &str) -> Result<Value> {
    let (file, bytes) = read(root, id)?;
    if file.kind != Kind::Text {
        return Err("Open this attachment with its native preview.".into());
    }
    Ok(json!({"text":std::str::from_utf8(&bytes).map_err(|_| "Unreadable text attachment.")?}))
}
pub(super) fn remove(root: &Path, id: &str) -> Result<()> {
    let mut first = None;
    for extension in ["jpg", "pdf", "txt", "json"] {
        match fs::remove_file(path(root, id, extension)?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                first = Some(e);
            }
        }
    }
    if first.is_some() {
        return Err("Could not remove a chat attachment.".into());
    }
    Ok(())
}

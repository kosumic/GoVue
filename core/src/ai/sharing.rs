//! One credential-free provider per bounded, versioned QR payload.
use super::{catalog, identity, providers};
use crate::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use qrcode::{Color, EcLevel, QrCode};
use serde_json::{Value, json};
use std::io::Write;

const PREFIX: &str = "govue:provider:1:";
// Bound density and decoding work; Q correction protects the rounded artwork.
const MAX_PAYLOAD: usize = 1000;
const MAX_DOCUMENT: usize = 256_000;
const INVALID: &str = "This QR code does not contain a supported GoVue provider.";
const TOO_LARGE: &str = "This provider has too many settings for one QR code. Share only the selected model, or use JSON export.";

pub(super) fn preview(p: &providers::Provider) -> Value {
    json!({"label":p.label,"kind":p.kind,"baseUrl":p.base_url,
        "protocol":catalog::protocol(p),"oauth":catalog::oauth(p),
        "model":p.model,"modelCount":p.models.len()})
}

pub(super) fn encode(p: &providers::Provider, selected_only: bool) -> Result<Value> {
    let mut p = p.clone();
    // The sender's local identity never crosses devices. Secrets live separately.
    p.id = "shared".into();
    if selected_only {
        p.models.retain(|m| m.id == p.model);
    }
    providers::validate_provider(&mut p)?;
    let document = serde_json::to_vec(&p).map_err(|_| INVALID)?;
    if document.len() > MAX_DOCUMENT {
        return Err(TOO_LARGE.into());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&document).map_err(|_| INVALID)?;
    let bytes = encoder.finish().map_err(|_| INVALID)?;
    let payload = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    if payload.len() > MAX_PAYLOAD {
        return Err(TOO_LARGE.into());
    }
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::Q)
        .map_err(|_| TOO_LARGE)?;
    let size = code.width();
    let modules: Vec<String> = code
        .to_colors()
        .chunks(size)
        .map(|row| {
            row.iter()
                .map(|c| if *c == Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect();
    Ok(json!({"payload":payload,"modules":modules,"provider":preview(&p)}))
}

pub(super) fn decode(payload: &str) -> Result<providers::Provider> {
    if payload.len() > MAX_PAYLOAD {
        return Err(INVALID.into());
    }
    let encoded = payload.strip_prefix(PREFIX).ok_or(INVALID)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| INVALID)?;
    let mut decoder = Decompress::new(true);
    let mut document = vec![0; MAX_DOCUMENT + 1];
    let status = decoder
        .decompress(&bytes, &mut document, FlushDecompress::Finish)
        .map_err(|_| INVALID)?;
    let length = usize::try_from(decoder.total_out()).map_err(|_| INVALID)?;
    if status != Status::StreamEnd
        || length > MAX_DOCUMENT
        || decoder.total_in() != bytes.len() as u64
    {
        return Err(INVALID.into());
    }
    document.truncate(length);
    // Strict provider/model schemas reject keys, tokens, headers and extra fields.
    let mut p: providers::Provider = serde_json::from_slice(&document).map_err(|_| INVALID)?;
    p.id = "shared".into();
    providers::validate_provider(&mut p)?;
    Ok(p)
}

pub(super) fn import(command: &Value, cfg: &mut providers::Config) -> Result<()> {
    if cfg.providers.len() >= 32 {
        return Err("At most 32 connections can be saved.".into());
    }
    let mut p = decode(crate::string(command, "payload")?)?;
    p.id = identity::random()?;
    if cfg.active_id.is_none() {
        cfg.active_id = Some(p.id.clone());
    }
    cfg.providers.push(p);
    Ok(())
}

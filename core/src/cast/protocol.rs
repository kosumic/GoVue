//! Cast V2 protobuf envelope. Adapted from LiTube / NewTube; see notices/.
use crate::Result;
use serde_json::Value;

pub const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const MEDIA: &str = "urn:x-cast:com.google.cast.media";
pub const SENDER: &str = "sender-0";
pub const APP: &str = "CC1AD845";

pub struct Message {
    pub source: String,
    pub destination: String,
    pub namespace: String,
    pub payload: Value,
}
fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn field(out: &mut Vec<u8>, key: u8, value: &str) {
    out.push(key);
    varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}
pub fn encode(destination: &str, namespace: &str, payload: &Value) -> Vec<u8> {
    let mut out = vec![8, 0];
    field(&mut out, 18, SENDER);
    field(&mut out, 26, destination);
    field(&mut out, 34, namespace);
    out.extend_from_slice(&[40, 0]);
    field(&mut out, 50, &payload.to_string());
    out
}
fn number(data: &[u8], at: &mut usize) -> Option<u64> {
    let mut n = 0u64;
    for shift in (0..=63).step_by(7) {
        let b = *data.get(*at)?;
        *at += 1;
        if shift == 63 && b > 1 {
            return None;
        }
        n |= ((b & 127) as u64) << shift;
        if b & 128 == 0 {
            return Some(n);
        }
    }
    None
}
pub fn decode(data: &[u8]) -> Result<Message> {
    let invalid = || "Invalid Cast message.".to_owned();
    if data.is_empty() || data.len() > 65536 {
        return Err(invalid());
    }
    let mut at = 0;
    let mut fields = std::collections::HashMap::new();
    let mut version = None;
    let mut kind = None;
    while at < data.len() {
        let key = number(data, &mut at).ok_or_else(invalid)?;
        let id = key >> 3;
        if id == 0 {
            return Err(invalid());
        }
        match key & 7 {
            0 => {
                let n = number(data, &mut at).ok_or_else(invalid)?;
                if id == 1 && version.replace(n).is_some() {
                    return Err(invalid());
                }
                if id == 5 && kind.replace(n).is_some() {
                    return Err(invalid());
                }
            }
            2 => {
                let length = usize::try_from(number(data, &mut at).ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
                let end = at
                    .checked_add(length)
                    .filter(|n| *n <= data.len())
                    .ok_or_else(invalid)?;
                if matches!(id, 2 | 3 | 4 | 6) && fields.insert(id, &data[at..end]).is_some() {
                    return Err(invalid());
                }
                at = end;
            }
            1 | 5 => {
                at = at
                    .checked_add(if key & 7 == 1 { 8 } else { 4 })
                    .filter(|n| *n <= data.len())
                    .ok_or_else(invalid)?;
            }
            _ => return Err(invalid()),
        }
    }
    if version != Some(0) || kind != Some(0) {
        return Err(invalid());
    }
    let text = |id| -> Result<String> {
        let s = std::str::from_utf8(fields.get(&id).ok_or_else(invalid)?).map_err(|_| invalid())?;
        if s.is_empty() || (id != 6 && (s.len() > 256 || s.chars().any(char::is_control))) {
            return Err(invalid());
        }
        Ok(s.into())
    };
    Ok(Message {
        source: text(2)?,
        destination: text(3)?,
        namespace: text(4)?,
        payload: serde_json::from_str(&text(6)?).map_err(|_| invalid())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn envelopes_keep_namespaces_and_payloads_and_skip_unknown_fields() {
        let payload =
            json!({"type":"LOAD","requestId":7,"media":{"metadata":{"title":"日本語 <title>"}}});
        let mut bytes = encode("transport-123", MEDIA, &payload);
        bytes.extend_from_slice(&[120, 1]);
        let message = decode(&bytes).unwrap();
        assert_eq!(message.source, SENDER);
        assert_eq!(message.destination, "transport-123");
        assert_eq!(message.namespace, MEDIA);
        assert_eq!(message.payload, payload);
    }
    #[test]
    fn corrupt_or_ambiguous_envelopes_cannot_reach_the_session() {
        let valid = encode("receiver-0", RECEIVER, &json!({"type":"GET_STATUS"}));
        for end in 0..valid.len() {
            assert!(decode(&valid[..end]).is_err());
        }
        let mut duplicate = valid.clone();
        duplicate.extend_from_slice(&[40, 0]);
        assert!(decode(&duplicate).is_err());
        let mut duplicate = valid.clone();
        field(&mut duplicate, 18, "another-sender");
        assert!(decode(&duplicate).is_err());
        let mut binary = valid.clone();
        binary[1] = 1;
        assert!(decode(&binary).is_err());
        assert!(decode(&vec![0; 65537]).is_err());
        assert!(decode(&[255; 11]).is_err());
    }
}

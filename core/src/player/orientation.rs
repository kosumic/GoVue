//! LiTube's physical-rotation rules. Transient surface state never reaches disk.
use crate::Result;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct Surface {
    fullscreen: bool,
    automatic: bool,
    suppressed: bool,
}
static SURFACES: OnceLock<Mutex<HashMap<String, Surface>>> = OnceLock::new();

pub(super) fn dispatch(command: &Value, root: &str) -> Result<Value> {
    let mut surfaces = SURFACES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Core state is unavailable. Restart GoVue.")?;
    let surface = surfaces.entry(root.into()).or_default();
    match crate::string(command, "op")? {
        "player_surface_reset" => *surface = Surface::default(),
        "player_fullscreen" => {
            let enabled = command["enabled"]
                .as_bool()
                .ok_or("Invalid fullscreen state.")?;
            if surface.fullscreen && !enabled {
                surface.suppressed = true;
            }
            surface.fullscreen = enabled;
            surface.automatic = false;
        }
        "player_orientation" => {
            let degrees = command["degrees"]
                .as_i64()
                .filter(|value| (-1..=359).contains(value))
                .ok_or("Invalid physical orientation.")?;
            let portrait =
                degrees >= 0 && (degrees <= 30 || degrees >= 330 || (150..=210).contains(&degrees));
            let landscape = (60..=120).contains(&degrees) || (240..=300).contains(&degrees);
            if portrait {
                surface.suppressed = false;
            }
            let locked = command["locked"] == true;
            if !surface.fullscreen
                && command["visible"] == true
                && command["systemAutoRotate"] == true
                && command["pip"] != true
                && command["mini"] != true
                && command["casting"] != true
                && !locked
                && landscape
                && !surface.suppressed
            {
                surface.fullscreen = true;
                surface.automatic = true;
            } else if surface.fullscreen && surface.automatic && !locked && portrait {
                surface.fullscreen = false;
                surface.automatic = false;
            }
        }
        _ => return Err("Unknown player orientation action.".into()),
    }
    let result = Ok(
        json!({"fullscreen":surface.fullscreen,"automatic":surface.automatic,"suppressed":surface.suppressed}),
    );
    drop(surfaces);
    result
}

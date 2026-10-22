//! Memory-only Cast sessions. Rust owns protocol, commands, media and revocation.
mod manifest;
mod network;
mod protocol;
use crate::{CastPlatform, Result, clean, string};
use protocol::{APP, CONNECTION, HEARTBEAT, MEDIA, RECEIVER, SENDER};
use serde_json::{Value, json};
use std::{
    net::IpAddr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

static PLATFORM: OnceLock<Box<dyn CastPlatform>> = OnceLock::new();
static CURRENT: Mutex<Option<Arc<Session>>> = Mutex::new(None);
static CHANGE: Mutex<()> = Mutex::new(());
pub(super) fn install(platform: Box<dyn CastPlatform>) {
    let _ = PLATFORM.set(platform);
}
pub(super) fn available() -> bool {
    PLATFORM.get().is_some()
}
fn platform() -> Result<&'static dyn CastPlatform> {
    PLATFORM
        .get()
        .map(std::convert::AsRef::as_ref)
        .ok_or("Casting is not available on this platform.".into())
}
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| "Could not create a Cast session.")?;
    Ok(crate::hex(&bytes))
}

struct View {
    phase: &'static str,
    error: Option<&'static str>,
    position: f64,
    duration: f64,
    speed: f64,
    sampled: Instant,
    acknowledged: bool,
    transport_id: String,
    app_session: String,
    media_session: Option<u64>,
    request: u64,
    load: u64,
    rate_sent: bool,
    last_inbound: Instant,
    last_poll: Instant,
    waiting: Instant,
}
struct Session {
    id: String,
    video_id: String,
    name: String,
    address: String,
    link: bool,
    device: Value,
    metadata: Mutex<Value>,
    alive: AtomicBool,
    socket: Mutex<String>,
    transport: Mutex<Option<Arc<network::Transport>>>,
    view: Mutex<View>,
    writer: Mutex<()>,
    created: Instant,
}
impl Session {
    fn active(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn position(view: &View) -> f64 {
        (view.position
            + if view.phase == "playing" {
                view.sampled.elapsed().as_secs_f64() * view.speed
            } else {
                0.0
            })
        .clamp(0.0, view.duration.max(view.position))
    }
    fn snapshot(&self) -> Result<Value> {
        let view = self
            .view
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        let transport = self
            .transport
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        let result = json!({"available":true,"id":self.id,"videoId":self.video_id,"device":self.name,"deviceId":if self.link {None}else{Some(format!("{}:{}",self.device["host"].as_str().unwrap_or(""),self.device["port"].as_u64().unwrap_or(0)))},"active":self.active(),"link":self.link,
            "phase":view.phase,"error":view.error,"position":Self::position(&view),"duration":view.duration,"speed":view.speed,"acknowledged":view.acknowledged,"linkConnected":self.link&&transport.as_ref().is_some_and(|t|t.connected()),
            "url":if self.link&&self.active(){transport.as_ref().map(|t|format!("{}player",t.base))}else{None}});
        drop(transport);
        drop(view);
        Ok(result)
    }
    fn finish(&self, error: Option<&'static str>) {
        if !self.alive.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut transport) = self.transport.lock()
            && let Some(transport) = transport.take()
        {
            transport.close();
        }
        let socket = self.socket.lock().ok().map(|socket| socket.clone());
        if let Some(socket) = socket
            && let Ok(p) = platform()
            && !socket.is_empty()
        {
            p.close(socket);
        }
        let Ok(mut view) = self.view.lock() else {
            return;
        };
        view.position = Self::position(&view);
        view.sampled = Instant::now();
        view.phase = if error.is_some() { "error" } else { "closed" };
        view.error = error;
    }
    fn send(&self, destination: &str, namespace: &str, payload: Value) -> Result<()> {
        let _write = self
            .writer
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        if !self.active() {
            return Err("The Cast session has ended.".into());
        }
        let socket = self
            .socket
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?
            .clone();
        if !socket.is_empty()
            && platform()?.write(socket, protocol::encode(destination, namespace, &payload))
        {
            Ok(())
        } else {
            Err("The Cast receiver disconnected.".into())
        }
    }
    fn request(&self, destination: &str, namespace: &str, mut payload: Value) -> Result<u64> {
        let id = {
            let mut view = self
                .view
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            view.request += 1;
            view.request
        };
        payload["requestId"] = json!(id);
        self.send(destination, namespace, payload)?;
        Ok(id)
    }
    fn stop_receiver(&self) {
        let app = self.view.lock().ok().map(|view| view.app_session.clone());
        if let Some(app) = app.filter(|app| !app.is_empty()) {
            let _ = self.request(
                "receiver-0",
                RECEIVER,
                json!({"type":"STOP","sessionId":app}),
            );
        }
        self.finish(None);
    }
    fn control(&self, command: &Value) -> Result<()> {
        if self.link {
            return Err("Control playback in the shared browser.".into());
        }
        let action = string(command, "action")?;
        {
            let mut view = self
                .view
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            if view.phase == "ended" && matches!(action, "play" | "seek") {
                let position = if action == "play" {
                    0.0
                } else {
                    command["position"]
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= view.duration)
                        .ok_or("Invalid Cast seek.")?
                };
                let destination = view.transport_id.clone();
                view.position = position;
                view.phase = "loading";
                view.waiting = Instant::now();
                view.sampled = Instant::now();
                view.rate_sent = false;
                view.acknowledged = false;
                view.media_session = None;
                view.request += 1;
                view.load = view.request;
                let id = view.load;
                drop(view);
                let media = self
                    .metadata
                    .lock()
                    .map_err(|_| "Cast state is unavailable. Restart GoVue.")?
                    .clone();
                return self.send(&destination,MEDIA,json!({"type":"LOAD","requestId":id,"media":media,"autoplay":true,"currentTime":position}));
            }
        }
        let (destination, mut message) = {
            let mut view = self
                .view
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            let media = view.media_session.ok_or("The receiver is still loading.")?;
            let mut message = match action {
                "play" => json!({"type":"PLAY"}),
                "pause" => json!({"type":"PAUSE"}),
                "seek" => {
                    let position = command["position"]
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= view.duration)
                        .ok_or("Invalid Cast seek.")?;
                    view.position = position;
                    view.sampled = Instant::now();
                    json!({"type":"SEEK","currentTime":position,"resumeState":if view.phase=="playing"{"PLAYBACK_START"}else{"PLAYBACK_PAUSE"}})
                }
                "speed" => {
                    let speed = command["speed"]
                        .as_f64()
                        .filter(|n| n.is_finite() && (0.25..=3.0).contains(n))
                        .ok_or("Invalid Cast speed.")?
                        .clamp(0.5, 2.0);
                    view.position = Self::position(&view);
                    view.sampled = Instant::now();
                    view.speed = speed;
                    json!({"type":"SET_PLAYBACK_RATE","playbackRate":speed})
                }
                _ => return Err("Unknown Cast control.".into()),
            };
            message["mediaSessionId"] = json!(media);
            (view.transport_id.clone(), message)
        };
        // Status polling also serves as command acknowledgement; no optimistic play state.
        self.request(&destination, MEDIA, message.take())?;
        Ok(())
    }
}
fn idle() -> Value {
    json!({"available":available(),"active":false,"id":null,"phase":"closed","acknowledged":false})
}
fn current() -> Result<Option<Arc<Session>>> {
    Ok(CURRENT
        .lock()
        .map_err(|_| "Cast state is unavailable. Restart GoVue.")?
        .clone())
}
fn stop_all() -> Result<()> {
    if let Some(session) = current()? {
        session.stop_receiver();
    }
    if let Ok(p) = platform() {
        let _ = p.activity(false);
    }
    Ok(())
}
pub(super) fn dispatch(command: &Value) -> Result<Value> {
    match command["op"].as_str().unwrap_or("") {
        "cast_status" => current()?.map_or_else(|| Ok(idle()), |s| s.snapshot()),
        "cast_stop_all" => {
            let _change = CHANGE
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            stop_all()?;
            Ok(idle())
        }
        "cast_stop" => {
            let _change = CHANGE
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            if let Some(session) =
                current()?.filter(|s| Some(s.id.as_str()) == command["id"].as_str())
            {
                stop_all()?;
                session.snapshot()
            } else {
                Ok(idle())
            }
        }
        "cast_sync" => {
            let _change = CHANGE
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            if current()?.is_some_and(|s| {
                s.active() && Some(s.video_id.as_str()) != command["videoId"].as_str()
            }) {
                stop_all()?;
            }
            current()?.map_or_else(|| Ok(idle()), |s| s.snapshot())
        }
        "cast_control" => {
            let session = current()?
                .filter(|s| s.active() && Some(s.id.as_str()) == command["id"].as_str())
                .ok_or("The Cast session has ended.")?;
            session.control(command)?;
            session.snapshot()
        }
        "cast_start" => start(command),
        "cast_replace" => {
            let owner = current()?
                .filter(|s| s.active() && !s.link && Some(s.id.as_str()) == command["id"].as_str())
                .ok_or("The Cast session has ended.")?;
            let mut next = command.clone();
            next["device"] = owner.device.clone();
            next["link"] = json!(false);
            start(&next)
        }
        _ => Err("Unknown Cast request.".into()),
    }
}
fn start(command: &Value) -> Result<Value> {
    let _change = CHANGE
        .lock()
        .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
    if command["op"] == "cast_replace"
        && !current()?.is_some_and(|s| s.active() && Some(s.id.as_str()) == command["id"].as_str())
    {
        return Err("The Cast session has ended.".into());
    }
    let p = platform()?;
    let address = p.wifi_address();
    let ip = network::private(&address).ok_or("Connect to a private Wi-Fi network to cast.")?;
    let video_id = string(&command["catalog"], "videoId")?;
    if command["catalog"]["live"] == true {
        return Err("Casting is unavailable for this live stream.".into());
    }
    if !crate::video_id(video_id) {
        return Err("Invalid Cast video.".into());
    }
    let link = command["link"] == true;
    let receiver = if link {
        None
    } else {
        let host = string(&command["device"], "host")?;
        let receiver = network::private(host).ok_or("Invalid Cast receiver address.")?;
        if receiver == ip {
            return Err("Choose a Cast receiver on this Wi-Fi network.".into());
        }
        let port = command["device"]["port"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port > 0)
            .ok_or("Invalid Cast receiver port.")?;
        Some((receiver, port))
    };
    let id = random()?;
    stop_all()?;
    if !p.activity(true) {
        return Err("Open GoVue and enable notifications before starting Cast.".into());
    }
    let now = Instant::now();
    let session = Arc::new(Session {
        id,
        video_id: video_id.into(),
        name: if link {
            "Shared player".into()
        } else {
            clean(command["device"]["name"].as_str().unwrap_or("Cast"))
                .chars()
                .take(200)
                .collect()
        },
        address,
        link,
        device: if link {
            Value::Null
        } else {
            json!({"host":command["device"]["host"],"port":command["device"]["port"],"name":command["device"]["name"].as_str().unwrap_or("Cast").chars().take(200).collect::<String>()})
        },
        metadata: Mutex::new(Value::Null),
        alive: AtomicBool::new(true),
        socket: Mutex::new(String::new()),
        transport: Mutex::new(None),
        writer: Mutex::new(()),
        created: now,
        view: Mutex::new(View {
            phase: "preparing",
            error: None,
            position: command["position"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(0.0),
            duration: 0.0,
            speed: command["speed"]
                .as_f64()
                .filter(|n| n.is_finite() && (0.25..=3.0).contains(n))
                .unwrap_or(1.0)
                .clamp(0.5, 2.0),
            sampled: now,
            acknowledged: false,
            transport_id: String::new(),
            app_session: String::new(),
            media_session: None,
            request: 0,
            load: 0,
            rate_sent: false,
            last_inbound: now,
            last_poll: now,
            waiting: now,
        }),
    });
    *CURRENT
        .lock()
        .map_err(|_| "Cast state is unavailable. Restart GoVue.")? = Some(session.clone());
    let work = session.clone();
    let command = command.clone();
    thread::spawn(move || {
        if let Err(error) = run(&work, &command, ip, receiver) {
            // Only fixed messages may leave the transport worker. Underlying
            // I/O errors can contain a signed source address or a capability.
            let message = match error.as_str() {
                "Could not prepare this video's Cast media. Try another quality." => {
                    "Could not prepare this video's Cast media. Try another quality."
                }
                "Could not open the Cast link on Wi-Fi." => {
                    "Could not open the Cast link on Wi-Fi."
                }
                "Could not start the Cast listener." => "Could not start the Cast listener.",
                "Cast authentication failed." => "Cast authentication failed.",
                _ => "Could not start Cast playback. Check Wi-Fi and try again.",
            };
            work.finish(Some(message));
        }
    });
    let weak = Arc::downgrade(&session);
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            let Some(session) = weak.upgrade() else { break };
            if !session.active() {
                break;
            }
            let network_changed = platform().is_ok_and(|p| p.wifi_address() != session.address);
            let ended = session.created.elapsed() > Duration::from_hours(4);
            if network_changed || ended {
                session.finish(Some(if network_changed {
                    "Wi-Fi changed. Start a new Cast session."
                } else {
                    "The Cast link expired. Start a new session."
                }));
                break;
            }
            if !session.link {
                let (timeout, poll, destination) = {
                    let Some(mut v) = session.view.lock().ok() else {
                        session.finish(Some("Cast state is unavailable. Restart GoVue."));
                        break;
                    };
                    let timeout = if matches!(v.phase, "connecting" | "loading") {
                        v.waiting.elapsed() > Duration::from_secs(20)
                    } else {
                        !v.transport_id.is_empty()
                            && v.last_inbound.elapsed() > Duration::from_secs(30)
                    };
                    let poll = v.last_poll.elapsed() > Duration::from_secs(5)
                        && !v.transport_id.is_empty();
                    if poll {
                        v.last_poll = Instant::now();
                    }
                    (timeout, poll, v.transport_id.clone())
                };
                if timeout {
                    session.finish(Some("The Cast receiver stopped responding."));
                    break;
                }
                if poll
                    && session
                        .send("receiver-0", HEARTBEAT, json!({"type":"PING"}))
                        .and_then(|_| {
                            session
                                .request(&destination, MEDIA, json!({"type":"GET_STATUS"}))
                                .map(|_| ())
                        })
                        .is_err()
                {
                    session.finish(Some("The Cast receiver disconnected."));
                    break;
                }
            }
        }
    });
    // Notification teardown must not stop a newer session.
    let weak = Arc::downgrade(&session);
    thread::spawn(move || {
        while weak.upgrade().is_some_and(|s| s.active()) {
            thread::sleep(Duration::from_secs(1));
        }
        let Ok(_change) = CHANGE.lock() else {
            return;
        };
        if current().is_ok_and(|session| !session.is_some_and(|s| s.active()))
            && let Ok(p) = platform()
        {
            let _ = p.activity(false);
        }
    });
    session.snapshot()
}
#[allow(
    clippy::float_cmp,
    clippy::too_many_lines,
    reason = "The receiver connection lifecycle stays together; an exact playback rate of 1.0 means its default."
)]
fn run(
    session: &Session,
    command: &Value,
    ip: IpAddr,
    receiver: Option<(IpAddr, u16)>,
) -> Result<()> {
    let bundle = manifest::build(command)
        .map_err(|_| "Could not prepare this video's Cast media. Try another quality.")?;
    if !session.active() {
        return Ok(());
    }
    let position = {
        let mut view = session
            .view
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        view.duration = bundle.duration;
        view.position = view.position.min(bundle.duration);
        view.position
    };
    // DeviceAuth must succeed before any media routes are opened for a receiver.
    if let Some((address, port)) = receiver {
        {
            let mut view = session
                .view
                .lock()
                .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
            view.phase = "connecting";
            view.waiting = Instant::now();
        }
        let socket = platform()?.connect(address.to_string(), port);
        if socket.is_empty() {
            return Err("Cast authentication failed.".into());
        }
        (*session
            .socket
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?)
        .clone_from(&socket);
        if !session.active() {
            platform()?.close(socket);
            return Ok(());
        }
    }
    let title = clean(command["catalog"]["title"].as_str().unwrap_or("GoVue"))
        .chars()
        .take(500)
        .collect::<String>();
    let transport = network::start(ip, receiver.map(|r| r.0), bundle, &title, position)?;
    {
        let mut current = session
            .transport
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        if !session.active() {
            transport.close();
            return Ok(());
        }
        *current = Some(transport.clone());
    }
    if session.link {
        session
            .view
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?
            .phase = "shared";
        return Ok(());
    }
    let metadata = json!({"contentId":transport.media,"streamType":"BUFFERED","contentType":transport.mime,"metadata":{"metadataType":0,"title":title,"subtitle":clean(command["catalog"]["author"].as_str().unwrap_or("")).chars().take(300).collect::<String>()}});
    *session
        .metadata
        .lock()
        .map_err(|_| "Cast state is unavailable. Restart GoVue.")? = metadata.clone();
    session.send(
        "receiver-0",
        CONNECTION,
        json!({"type":"CONNECT","origin":{}}),
    )?;
    session.request("receiver-0", RECEIVER, json!({"type":"LAUNCH","appId":APP}))?;
    let socket = session
        .socket
        .lock()
        .map_err(|_| "Cast state is unavailable. Restart GoVue.")?
        .clone();
    while session.active() {
        let frame = platform()?.read(socket.clone());
        if frame.is_empty() {
            if session.active() {
                return Err("Cast connection closed.".into());
            }
            break;
        }
        let message = protocol::decode(&frame)?;
        if !matches!(message.destination.as_str(), SENDER | "*") {
            continue;
        }
        let mut view = session
            .view
            .lock()
            .map_err(|_| "Cast state is unavailable. Restart GoVue.")?;
        if message.source != "receiver-0" && message.source != view.transport_id {
            continue;
        }
        view.last_inbound = Instant::now();
        let kind = message.payload["type"].as_str().unwrap_or("");
        match message.namespace.as_str() {
            HEARTBEAT if kind == "PING" => {
                drop(view);
                session.send(&message.source, HEARTBEAT, json!({"type":"PONG"}))?;
            }
            CONNECTION if kind == "CLOSE" => {
                drop(view);
                session.finish(None);
                break;
            }
            RECEIVER if message.source == "receiver-0" && kind == "LAUNCH_ERROR" => {
                return Err("Cast launch failed.".into());
            }
            RECEIVER if message.source == "receiver-0" && kind == "RECEIVER_STATUS" => {
                let app = message.payload["status"]["applications"]
                    .as_array()
                    .and_then(|apps| apps.iter().take(30).find(|a| a["appId"] == APP));
                let Some(app) = app else {
                    if !view.transport_id.is_empty() {
                        drop(view);
                        session.finish(None);
                        break;
                    }
                    continue;
                };
                if !view.transport_id.is_empty() {
                    continue;
                }
                let valid = |s: Option<&str>| {
                    s.filter(|s| {
                        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
                    })
                    .map(str::to_owned)
                };
                view.transport_id =
                    valid(app["transportId"].as_str()).ok_or("Missing Cast transport.")?;
                view.app_session =
                    valid(app["sessionId"].as_str()).ok_or("Missing Cast app session.")?;
                let destination = view.transport_id.clone();
                view.phase = "loading";
                view.waiting = Instant::now();
                // Reserve LOAD's correlation id before the receiver can answer.
                view.request += 1;
                view.load = view.request;
                let id = view.load;
                drop(view);
                session.send(
                    &destination,
                    CONNECTION,
                    json!({"type":"CONNECT","origin":{}}),
                )?;
                session.send(&destination,MEDIA,json!({"type":"LOAD","requestId":id,"autoplay":true,"currentTime":position,"media":metadata}))?;
            }
            MEDIA if message.source == view.transport_id => {
                let request = message.payload["requestId"].as_u64().unwrap_or(0);
                if request != 0 && (request < view.load || request > view.request) {
                    continue;
                }
                if matches!(kind, "LOAD_FAILED" | "LOAD_CANCELLED") && request == view.load {
                    return Err("The receiver rejected the video.".into());
                }
                if kind != "MEDIA_STATUS" {
                    continue;
                }
                let Some(status) = message.payload["status"].as_array().and_then(|s| s.first())
                else {
                    continue;
                };
                let content = status["media"]["contentId"].as_str();
                if content.is_some_and(|id| id != transport.media)
                    || (!view.acknowledged
                        && request != view.load
                        && content != Some(transport.media.as_str()))
                {
                    continue;
                }
                let media = status["mediaSessionId"]
                    .as_u64()
                    .filter(|n| i32::try_from(*n).is_ok())
                    .ok_or("Missing Cast media session.")?;
                if view.media_session.is_some_and(|n| n != media)
                    && content != Some(transport.media.as_str())
                {
                    continue;
                }
                view.media_session = Some(media);
                view.acknowledged = true;
                view.position = status["currentTime"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .unwrap_or(view.position)
                    .min(view.duration);
                view.sampled = Instant::now();
                view.phase = match status["playerState"].as_str() {
                    Some("PLAYING") => "playing",
                    Some("PAUSED") => "paused",
                    Some("BUFFERING") => "buffering",
                    Some("IDLE") if status["idleReason"] == "FINISHED" => "ended",
                    Some("IDLE") if status["idleReason"] == "ERROR" => {
                        return Err("Receiver media error.".into());
                    }
                    _ => view.phase,
                };
                let rate = (!view.rate_sent && matches!(view.phase, "playing" | "paused"))
                    .then_some(view.speed);
                if rate.is_some() {
                    view.rate_sent = true;
                }
                drop(view);
                if let Some(speed) = rate.filter(|n| *n != 1.0) {
                    session.control(&json!({"action":"speed","speed":speed}))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

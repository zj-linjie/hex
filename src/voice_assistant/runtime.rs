use super::{Snapshot, Status, Turn, snapshot, update};
mod codec;
mod heartbeat;
use codec::{Decoder, Encoder};
use heartbeat::{Liveness, service_upload, set_read_timeout};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

const FRAME: usize = 960;
const MAX_SAMPLES: usize = 16_000 * 60;
const MAX_TEXT: usize = 16_384;
static GENERATION: AtomicU64 = AtomicU64::new(0);
static TEMP_NONCE: AtomicU64 = AtomicU64::new(0);
static DNS_BUSY: AtomicBool = AtomicBool::new(false);
static PLAYBACK: AtomicBool = AtomicBool::new(true);
static PLAYBACK_VERSION: AtomicU64 = AtomicU64::new(0);
static PLAYER: Mutex<Option<Arc<rodio::Player>>> = Mutex::new(None);
static TX: OnceLock<SyncSender<(u64, Command)>> = OnceLock::new();

enum SocketState {
    Idle,
    Active,
}
pub(super) enum Command {
    Enable(bool),
    Playback(bool),
    Reconnect,
    Begin,
    Submit(Vec<f32>),
    Cancel,
}
#[derive(Deserialize)]
struct Identity {
    device_id: String,
    client_id: String,
    websocket: Endpoint,
}
#[derive(Deserialize)]
struct Endpoint {
    url: String,
    token: String,
}
fn identity_path() -> Result<std::path::PathBuf, String> {
    crate::app_paths::support_dir()
        .map(|p| p.join("voice-assistant-identity.json"))
        .map_err(|_| "Application Support is unavailable".into())
}
fn parse_identity(bytes: &[u8]) -> Result<Identity, String> {
    if bytes.len() > 32_768 {
        return Err("Identity file is too large".into());
    }
    let id: Identity =
        serde_json::from_slice(bytes).map_err(|_| "Invalid assistant identity file".to_string())?;
    let url = url::Url::parse(&id.websocket.url)
        .map_err(|_| "Invalid assistant server address".to_string())?;
    if url.scheme() != "wss"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || [
            id.device_id.as_str(),
            id.client_id.as_str(),
            id.websocket.token.as_str(),
        ]
        .iter()
        .any(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
    {
        return Err("Identity must contain an activated secure assistant connection".into());
    }
    Ok(id)
}
pub(super) fn import_identity(path: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Could not open identity file".to_string())?
        .take(32_769)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read identity file".to_string())?;
    parse_identity(&bytes)?;
    let dest = identity_path()?;
    let dir = dest.parent().ok_or("Application Support is unavailable")?;
    fs::create_dir_all(dir).map_err(|_| "Could not create Application Support directory")?;
    let temp = dir.join(format!(
        ".voice-assistant-identity-{}-{}.tmp",
        std::process::id(),
        TEMP_NONCE.fetch_add(1, Ordering::SeqCst)
    ));
    let result: Result<(), String> = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "Could not protect assistant identity")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "Could not save assistant identity")?;
        fs::rename(&temp, &dest).map_err(|_| "Could not install assistant identity")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    if snapshot().enabled {
        interrupt(Command::Reconnect);
    }
    Ok(())
}
fn sender() -> &'static SyncSender<(u64, Command)> {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("voice-assistant".into())
            .spawn(move || worker(rx))
            .expect("assistant worker");
        tx
    })
}
pub(super) fn signal(command: Command) {
    if sender()
        .try_send((GENERATION.load(Ordering::SeqCst), command))
        .is_err()
    {
        update(|s| s.status = Status::Error("Assistant is busy; try again".into()));
    }
}
pub(super) fn submit(samples: Vec<f32>) -> Result<u64, String> {
    if samples.len() < 1600
        || samples.len() > MAX_SAMPLES
        || samples.iter().any(|s| !s.is_finite())
        || samples.iter().fold(0_f32, |m, s| m.max(s.abs())) < 0.005
    {
        return Err("Invalid, silent, or excessively long recording".into());
    }
    let mut state = super::state()
        .lock()
        .map_err(|_| "Assistant is unavailable")?;
    if !state.enabled {
        return Err("Voice assistant is disabled".into());
    }
    let receipt = GENERATION.load(Ordering::SeqCst);
    sender()
        .try_send((receipt, Command::Submit(samples)))
        .map_err(|_| "Assistant is busy; try again")?;
    state.turn_id = Some(receipt);
    state.status = Status::Waiting;
    state.audio_error = None;
    Ok(receipt)
}
pub(super) fn mute_now() {
    if let Ok(mut player) = PLAYER.lock() {
        if let Some(player) = player.take() {
            player.stop();
        }
    }
}
pub(super) fn playback_changed(enabled: bool) {
    PLAYBACK.store(enabled, Ordering::SeqCst);
    PLAYBACK_VERSION.fetch_add(1, Ordering::SeqCst);
    if !enabled {
        mute_now();
    }
}
pub(super) fn interrupt(command: Command) {
    update(|_| {
        GENERATION.fetch_add(1, Ordering::SeqCst);
    });
    mute_now();
    signal(command);
}
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
fn connect() -> Result<(Socket, String, u32), String> {
    let mut bytes = Vec::new();
    fs::File::open(identity_path()?)
        .map_err(|_| "Import an activated assistant identity".to_string())?
        .take(32_769)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read assistant identity".to_string())?;
    connect_identity(&bytes)
}
fn connect_identity(bytes: &[u8]) -> Result<(Socket, String, u32), String> {
    let id = parse_identity(bytes)?;
    let mut req = id
        .websocket
        .url
        .as_str()
        .into_client_request()
        .map_err(|_| "Invalid assistant server address")?;
    for (k, v) in [
        ("Authorization", format!("Bearer {}", id.websocket.token)),
        ("Protocol-Version", "1".to_string()),
        ("Device-Id", id.device_id),
        ("Client-Id", id.client_id),
    ] {
        req.headers_mut().insert(
            tungstenite::http::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| "Invalid identity")?,
            v.parse().map_err(|_| "Invalid identity")?,
        );
    }
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(64 * 1024))
        .max_frame_size(Some(64 * 1024));
    // Resolve on a separately bounded thread; at most one stuck system resolver
    // can exist, even if the operating system's resolver never returns.
    let url = url::Url::parse(&id.websocket.url).map_err(|_| "Invalid assistant server address")?;
    let host = url
        .host_str()
        .ok_or("Invalid assistant server address")?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or("Invalid assistant server address")?;
    if DNS_BUSY.swap(true, Ordering::SeqCst) {
        return Err("Assistant name lookup is still pending".into());
    }
    let (dns_tx, dns_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let addresses = (host.as_str(), port)
            .to_socket_addrs()
            .map(|iter| iter.take(16).collect::<Vec<_>>());
        DNS_BUSY.store(false, Ordering::SeqCst);
        let _ = dns_tx.try_send(addresses);
    });
    let addresses = dns_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Assistant name lookup timed out")?
        .map_err(|_| "Could not resolve assistant server")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut tcp = None;
    for addr in addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(stream) = TcpStream::connect_timeout(&addr, remaining) {
            tcp = Some(stream);
            break;
        }
    }
    let tcp = tcp.ok_or("Could not connect to assistant server")?;
    tcp.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "Could not configure connection")?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "Could not configure connection")?;
    // A peer can drip handshake bytes indefinitely despite per-read timeouts.
    // The watchdog closes this socket after the total handshake deadline.
    let watchdog_socket = tcp
        .try_clone()
        .map_err(|_| "Could not configure connection")?;
    let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);
    std::thread::spawn(move || {
        if done_rx.recv_timeout(Duration::from_secs(10)).is_err() {
            let _ = watchdog_socket.shutdown(Shutdown::Both);
        }
    });
    let handshake = tungstenite::client_tls_with_config(req, tcp, Some(config), None);
    let _ = done_tx.try_send(());
    let (mut ws, _) = handshake.map_err(|_| "Could not connect to assistant server")?;
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::NativeTls(s) => s.get_mut(),
        _ => return Err("Unsupported connection".into()),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|_| "Could not configure connection")?;
    tcp.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "Could not configure connection")?;
    send_json(
        &mut ws,
        json!({"type":"hello","version":1,"transport":"websocket","audio_params":{"format":"opus","sample_rate":16000,"channels":1,"frame_duration":60}}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if Instant::now() > deadline {
            return Err("Assistant hello timed out".into());
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(&t).map_err(|_| "Invalid assistant hello")?;
                if v["type"] == "hello" {
                    let rate = v["audio_params"]["sample_rate"].as_u64().unwrap_or(24000) as u32;
                    let channels = v["audio_params"]["channels"].as_u64().unwrap_or(1);
                    let format = v["audio_params"]["format"].as_str().unwrap_or("opus");
                    let sid = v["session_id"].as_str().unwrap_or("");
                    if ![8000, 12000, 16000, 24000, 48000].contains(&rate)
                        || channels != 1
                        || format != "opus"
                        || sid.is_empty()
                        || sid.len() > 1024
                    {
                        return Err("Unsupported assistant audio format".into());
                    }
                    return Ok((ws, sid.to_string(), rate));
                }
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                ()
            }
            _ => return Err("Assistant connection closed during hello".into()),
        }
    }
}
fn send_json(ws: &mut Socket, v: Value) -> Result<(), String> {
    ws.send(Message::Text(v.to_string().into()))
        .map_err(|_| "Assistant connection failed".into())
}
fn best_effort_abort(ws: &mut Socket, sid: &str) {
    let tcp = match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::NativeTls(s) => s.get_mut(),
        _ => return,
    };
    let _ = tcp.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = send_json(
        ws,
        json!({"session_id":sid,"type":"abort","reason":"user_stopped"}),
    );
}
fn append_text(target: &mut String, text: &str) {
    let remaining = MAX_TEXT.saturating_sub(target.len());
    let end = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .take_while(|i| *i <= remaining)
        .last()
        .unwrap_or(0);
    target.push_str(&text[..end]);
}
#[derive(Debug, PartialEq, Eq)]
enum Event {
    None,
    Start,
    Stop,
    Goodbye,
}
// Text is independent of playback; obsolete connections cannot mutate the conversation.
fn apply_event(s: &mut Snapshot, v: &Value, current: bool) -> Event {
    if !current || !s.enabled {
        return Event::None;
    }
    match (v["type"].as_str(), v["state"].as_str()) {
        (Some("stt"), _) => {
            if let (Some(turn), Some(text)) = (s.turns.last_mut(), v["text"].as_str()) {
                append_text(&mut turn.transcript, text);
            }
            Event::None
        }
        (Some("tts"), Some("sentence_start")) => {
            if let (Some(turn), Some(text)) = (s.turns.last_mut(), v["text"].as_str()) {
                append_text(&mut turn.reply, text);
            }
            Event::None
        }
        (Some("tts"), Some("start")) => {
            s.status = Status::Speaking;
            Event::Start
        }
        (Some("tts"), Some("stop")) => {
            s.status = Status::Ready;
            Event::Stop
        }
        (Some("goodbye"), _) => {
            s.status = Status::Error("Assistant session ended".into());
            Event::Goodbye
        }
        _ => Event::None,
    }
}
fn worker(rx: mpsc::Receiver<(u64, Command)>) {
    let mut enabled = false;
    let mut connection: Option<(Socket, String, Decoder)> = None;
    let mut active = SocketState::Idle;
    let mut generation = GENERATION.load(Ordering::SeqCst);
    let mut speaker: Option<(rodio::MixerDeviceSink, Arc<rodio::Player>)> = None;
    let mut accept_audio = false;
    let mut deadline: Option<Instant> = None;
    let mut liveness = Liveness::new();
    let mut pending = VecDeque::new();
    let mut output_failed = false;
    let mut playback_version = PLAYBACK_VERSION.load(Ordering::SeqCst);
    loop {
        let desired = super::state().lock().map(|s| s.enabled).unwrap_or(false);
        if enabled && !desired {
            if let Some((ws, sid, _)) = &mut connection {
                best_effort_abort(ws, sid);
            }
            connection = None;
            speaker = None;
            active = SocketState::Idle;
            deadline = None;
            enabled = false;
        }
        let version = PLAYBACK_VERSION.load(Ordering::SeqCst);
        if version != playback_version {
            playback_version = version;
            output_failed = false;
        }
        if !PLAYBACK.load(Ordering::SeqCst) {
            speaker = None;
        }
        match rx.try_recv() {
            Ok((command_generation, command)) => {
                if command_generation != GENERATION.load(Ordering::SeqCst)
                    && !matches!(command, Command::Enable(_) | Command::Playback(_))
                {
                    continue;
                }
                match command {
                    Command::Enable(_on) => {
                        generation = command_generation;
                        enabled = desired;
                        if !enabled {
                            if let Some((ws, sid, _)) = &mut connection {
                                best_effort_abort(ws, sid);
                            }
                            connection = None;
                            speaker = None;
                            active = SocketState::Idle;
                            deadline = None;
                        } else if connection.is_none() {
                            establish(&mut connection, generation);
                            liveness = Liveness::new();
                        }
                    }
                    Command::Playback(on) => {
                        if !on {
                            speaker = None;
                        }
                    }
                    Command::Reconnect | Command::Begin | Command::Cancel => {
                        generation = command_generation;
                        let interrupting =
                            !matches!(active, SocketState::Idle) || speaker.is_some();
                        if interrupting {
                            if let Some((ws, sid, _)) = &mut connection {
                                best_effort_abort(ws, sid);
                            }
                            connection = None;
                        }
                        speaker = None;
                        accept_audio = false;
                        active = SocketState::Idle;
                        deadline = None;
                        match command {
                            Command::Cancel => update(|s| {
                                if s.enabled {
                                    s.status = Status::Cancelled
                                }
                            }),
                            Command::Reconnect if enabled => {
                                connection = None;
                                establish(&mut connection, generation);
                                liveness = Liveness::new();
                            }
                            Command::Begin if enabled => {
                                if connection.is_none() {
                                    establish(&mut connection, generation);
                                    liveness = Liveness::new();
                                }
                                if connection.is_some() {
                                    update(|s| {
                                        if s.turn_id != Some(generation) {
                                            s.status = Status::Listening;
                                        }
                                    });
                                }
                            }
                            _ => (),
                        }
                    }
                    Command::Submit(samples) if enabled => {
                        if connection.is_none() {
                            update(|s| {
                                s.status = Status::Error(
                                    "Assistant is not connected; reconnect and try again".into(),
                                )
                            });
                            continue;
                        }
                        let (ws, sid, _) = connection.as_mut().expect("checked");
                        update(|s| {
                            if s.turns.len() >= 20 {
                                s.turns.remove(0);
                            }
                            s.turns.push(Turn::default());
                            s.status = Status::Waiting;
                        });
                        active = SocketState::Active;
                        output_failed = false;
                        pending.clear();
                        let result =
                            transmit(ws, sid, &samples, generation, &mut liveness, &mut pending);
                        if let Err(error) = result {
                            if generation != GENERATION.load(Ordering::SeqCst) {
                                best_effort_abort(ws, sid);
                            }
                            connection = None;
                            active = SocketState::Idle;
                            if generation == GENERATION.load(Ordering::SeqCst) {
                                update(|s| s.status = Status::Error(error));
                            }
                        } else {
                            deadline = Some(Instant::now() + Duration::from_secs(120));
                        }
                    }
                    Command::Submit(_) => (),
                }
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => (),
        }
        if generation != GENERATION.load(Ordering::SeqCst) {
            // Interrupts take effect even if the bounded command queue is full.
            if !matches!(active, SocketState::Idle) || speaker.is_some() {
                if let Some((ws, sid, _)) = &mut connection {
                    best_effort_abort(ws, sid);
                }
                connection = None;
            }
            speaker = None;
            accept_audio = false;
            active = SocketState::Idle;
            deadline = None;
            generation = GENERATION.load(Ordering::SeqCst);
            continue;
        }
        if let Some((ws, _, decoder)) = connection.as_mut() {
            if let Err(error) = liveness.tick(ws) {
                connection = None;
                active = SocketState::Idle;
                deadline = None;
                update(|s| {
                    if s.enabled && generation == GENERATION.load(Ordering::SeqCst) {
                        s.status = Status::Error(error);
                    }
                });
                continue;
            }
            let incoming = pending.pop_front().map(Ok).unwrap_or_else(|| ws.read());
            match incoming {
                Ok(Message::Text(text)) if matches!(active, SocketState::Active) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        let mut event = Event::None;
                        update(|s| {
                            event =
                                apply_event(s, &v, generation == GENERATION.load(Ordering::SeqCst));
                        });
                        match event {
                            Event::Start => accept_audio = true,
                            Event::Stop => {
                                accept_audio = false;
                                active = SocketState::Idle;
                                deadline = None;
                            }
                            Event::Goodbye => {
                                connection = None;
                                active = SocketState::Idle;
                            }
                            Event::None => (),
                        }
                    }
                }
                Ok(Message::Binary(packet))
                    if accept_audio && PLAYBACK.load(Ordering::SeqCst) && !output_failed =>
                {
                    match decoder.decode(&packet) {
                        Ok(samples) => {
                            if speaker.is_none() {
                                match rodio::DeviceSinkBuilder::open_default_sink() {
                                    Ok(sink) => {
                                        let player =
                                            Arc::new(rodio::Player::connect_new(sink.mixer()));
                                        if let Ok(mut slot) = PLAYER.lock() {
                                            if PLAYBACK.load(Ordering::SeqCst)
                                                && generation == GENERATION.load(Ordering::SeqCst)
                                            {
                                                *slot = Some(player.clone());
                                                speaker = Some((sink, player));
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        output_failed = true;
                                        update(|s| {
                                            s.audio_error = Some("Could not open audio output; text remains available".into())
                                        });
                                    }
                                }
                            }
                            if let Some((_, player)) = &speaker {
                                if player.len() < 100 {
                                    // Serialize appends with mute/interrupt so no packet can
                                    // restart a player that was just stopped.
                                    if let Ok(slot) = PLAYER.lock() {
                                        if slot.as_ref().is_some_and(|p| Arc::ptr_eq(p, player))
                                            && PLAYBACK.load(Ordering::SeqCst)
                                            && generation == GENERATION.load(Ordering::SeqCst)
                                        {
                                            player.append(rodio::buffer::SamplesBuffer::new(
                                                std::num::NonZeroU16::new(1).unwrap(),
                                                std::num::NonZeroU32::new(decoder.1).unwrap(),
                                                samples,
                                            ));
                                        }
                                    }
                                } else {
                                    speaker = None;
                                    output_failed = true;
                                    mute_now();
                                    update(|s| {
                                        s.audio_error = Some(
                                            "Audio output fell behind; text remains available"
                                                .into(),
                                        )
                                    });
                                }
                            }
                        }
                        Err(error) => {
                            output_failed = true;
                            update(|s| s.audio_error = Some(error));
                        }
                    }
                }
                Ok(Message::Pong(payload)) => liveness.pong(&payload),
                Ok(Message::Ping(_)) => {
                    let _ = ws.flush();
                }
                Ok(Message::Close(_)) => {
                    connection = None;
                    active = SocketState::Idle;
                    update(|s| {
                        s.status =
                            Status::Error("Assistant disconnected; reconnect to continue".into())
                    });
                }
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    ()
                }
                Err(_) => {
                    connection = None;
                    active = SocketState::Idle;
                    update(|s| {
                        s.status = Status::Error(
                            "Assistant disconnected; the last operation was not retried".into(),
                        )
                    });
                }
                _ => (),
            }
        } else {
            std::thread::sleep(Duration::from_millis(25));
        }
        if matches!(active, SocketState::Idle) && speaker.as_ref().is_some_and(|(_, p)| p.empty()) {
            speaker = None;
            if let Ok(mut slot) = PLAYER.lock() {
                *slot = None;
            }
        }
        if deadline.is_some_and(|d| Instant::now() > d) {
            connection = None;
            speaker = None;
            active = SocketState::Idle;
            deadline = None;
            update(|s| {
                s.status = Status::Error(
                    "Assistant response timed out; the operation was not retried".into(),
                )
            });
        }
    }
}
fn establish(connection: &mut Option<(Socket, String, Decoder)>, generation: u64) {
    update(|s| {
        if s.enabled
            && s.turn_id != Some(generation)
            && generation == GENERATION.load(Ordering::SeqCst)
        {
            s.status = Status::Connecting;
        }
    });
    let result = connect().and_then(|(ws, sid, rate)| Ok((ws, sid, Decoder::new(rate)?)));
    if generation != GENERATION.load(Ordering::SeqCst) || !snapshot().enabled {
        return;
    }
    match result {
        Ok(c) => {
            *connection = Some(c);
            update(|s| {
                if s.enabled
                    && s.turn_id != Some(generation)
                    && generation == GENERATION.load(Ordering::SeqCst)
                {
                    s.status = Status::Ready;
                }
            });
        }
        Err(e) => update(|s| {
            if !s.enabled || generation != GENERATION.load(Ordering::SeqCst) {
                return;
            }
            s.status = if e == "Import an activated assistant identity" {
                Status::NeedsIdentity
            } else {
                Status::Error(e)
            }
        }),
    }
}
fn transmit(
    ws: &mut Socket,
    sid: &str,
    samples: &[f32],
    generation: u64,
    live: &mut Liveness,
    pending: &mut VecDeque<Message>,
) -> Result<(), String> {
    let mut enc = Encoder::new()?;
    if generation != GENERATION.load(Ordering::SeqCst) {
        return Err("Cancelled".into());
    }
    send_json(
        ws,
        json!({"session_id":sid,"type":"listen","state":"start","mode":"manual"}),
    )?;
    set_read_timeout(ws, Duration::from_millis(5))?;
    let result = (|| {
        let start = Instant::now();
        for (i, chunk) in samples.chunks(FRAME).enumerate() {
            while Instant::now() < start + Duration::from_millis(i as u64 * 60) {
                if generation != GENERATION.load(Ordering::SeqCst) {
                    return Err("Cancelled".into());
                }
                service_upload(ws, live, pending)?;
            }
            if generation != GENERATION.load(Ordering::SeqCst) {
                return Err("Cancelled".into());
            }
            let mut frame = [0.0; FRAME];
            frame[..chunk.len()].copy_from_slice(chunk);
            ws.send(Message::Binary(enc.encode(&frame)?.into()))
                .map_err(|_| "Assistant connection failed; the operation was not retried")?;
        }
        send_json(ws, json!({"session_id":sid,"type":"listen","state":"stop"}))
    })();
    let restore = set_read_timeout(ws, Duration::from_millis(100));
    result.and(restore)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_diagnostics_cannot_log_websocket_credentials_or_payloads() {
        const CHILD: &str = "HEX_ASSISTANT_LOG_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "voice_assistant::runtime::tests::trace_diagnostics_cannot_log_websocket_credentials_or_payloads"])
                .env(CHILD, "1").status().unwrap();
            assert!(result.success());
            return;
        }
        struct Capture(Mutex<String>);
        impl log::Log for Capture {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, record: &log::Record<'_>) {
                self.0.lock().unwrap().push_str(&record.args().to_string());
            }
            fn flush(&self) {}
        }
        static CAPTURE: Capture = Capture(Mutex::new(String::new()));
        log::set_logger(&CAPTURE).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
        assert!(log::STATIC_MAX_LEVEL <= log::LevelFilter::Info);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let _ = ws.read();
            ws.send(Message::Text("fake-reply-sensitive".into()))
                .unwrap();
        });
        let mut req = format!("ws://{address}").into_client_request().unwrap();
        req.headers_mut().insert(
            "Authorization",
            "Bearer fake-token-sensitive".parse().unwrap(),
        );
        let tcp = TcpStream::connect(address).unwrap();
        let (mut ws, _) = tungstenite::client(req, tcp).unwrap();
        ws.send(Message::Text("fake-transcript-sensitive".into()))
            .unwrap();
        let _ = ws.read();
        server.join().unwrap();
        let captured = CAPTURE.0.lock().unwrap();
        assert!(!captured.contains("fake-token-sensitive"));
        assert!(!captured.contains("fake-transcript-sensitive"));
        assert!(!captured.contains("fake-reply-sensitive"));
    }
    #[test]
    fn disabled_settings_do_not_start_worker() {
        const CHILD: &str = "HEX_ASSISTANT_WORKER_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "voice_assistant::runtime::tests::disabled_settings_do_not_start_worker",
                ])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(result.success());
            return;
        }
        assert!(TX.get().is_none());
        super::super::set_enabled(false);
        super::super::set_playback(false);
        assert!(TX.get().is_none());
        assert!(!snapshot().enabled);
    }
    #[test]
    fn muted_text_survives_and_late_or_disabled_events_are_ignored() {
        let mut state = Snapshot {
            enabled: true,
            playback: false,
            status: Status::Waiting,
            turns: vec![Turn::default()],
            turn_id: Some(1),
            audio_error: None,
        };
        apply_event(&mut state, &json!({"type":"stt","text":"question"}), true);
        apply_event(
            &mut state,
            &json!({"type":"tts","state":"sentence_start","text":"answer"}),
            true,
        );
        assert_eq!(state.turns[0].transcript, "question");
        assert_eq!(state.turns[0].reply, "answer");
        apply_event(
            &mut state,
            &json!({"type":"tts","state":"sentence_start","text":"stale"}),
            false,
        );
        assert_eq!(state.turns[0].reply, "answer");
        state.enabled = false;
        assert_eq!(
            apply_event(&mut state, &json!({"type":"tts","state":"start"}), true),
            Event::None
        );
        assert_eq!(state.status, Status::Waiting);
    }
    #[test]
    fn interruption_prevents_audio_submission() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            assert!(ws.read().is_err());
        });
        let tcp = TcpStream::connect(addr).unwrap();
        let (mut ws, _) =
            tungstenite::client(format!("ws://{addr}"), MaybeTlsStream::Plain(tcp)).unwrap();
        let stale = GENERATION.load(Ordering::SeqCst).wrapping_sub(1);
        assert!(
            transmit(
                &mut ws,
                "session",
                &[0.1; FRAME],
                stale,
                &mut Liveness::new(),
                &mut VecDeque::new()
            )
            .is_err()
        );
        drop(ws);
        server.join().unwrap();
    }
    #[test]
    fn rejects_insecure_or_unactivated_identity() {
        assert!(parse_identity(br#"{"device_id":"a","client_id":"b","websocket":{"url":"ws://example.com","token":"c"}}"#).is_err());
        assert!(parse_identity(br#"{"device_id":"a","client_id":"b","websocket":{"url":"wss://example.com","token":""}}"#).is_err());
    }
    #[test]
    fn sends_manual_listen_and_paced_opus_to_mock_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let start = ws.read().unwrap().into_text().unwrap();
            let start: Value = serde_json::from_str(&start).unwrap();
            assert_eq!(start["type"], "listen");
            assert_eq!(start["state"], "start");
            assert_eq!(start["mode"], "manual");
            let mut decoder = Decoder::new(16000).unwrap();
            let bytes = ws.read().unwrap().into_data();
            assert_eq!(decoder.decode(&bytes).unwrap().len(), FRAME);
            ws.send(Message::Ping(vec![7].into())).unwrap();
            let pong = ws.read().unwrap();
            assert!(matches!(pong, Message::Pong(p) if p.as_ref() == [7]));
            let bytes = ws.read().unwrap().into_data();
            assert_eq!(decoder.decode(&bytes).unwrap().len(), FRAME);
            let stop = ws.read().unwrap().into_text().unwrap();
            let stop: Value = serde_json::from_str(&stop).unwrap();
            assert_eq!(stop["state"], "stop");
        });
        let tcp = TcpStream::connect(address).unwrap();
        let (mut socket, _) =
            tungstenite::client(format!("ws://{address}"), MaybeTlsStream::Plain(tcp)).unwrap();
        let generation = GENERATION.load(Ordering::SeqCst);
        let started = Instant::now();
        transmit(
            &mut socket,
            "test-session",
            &vec![0.1; FRAME * 2],
            generation,
            &mut Liveness::new(),
            &mut VecDeque::new(),
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(50));
        server.join().unwrap();
    }
    #[test]
    fn opus_roundtrip_and_text_bound() {
        let mut enc = Encoder::new().unwrap();
        let mut dec = Decoder::new(16000).unwrap();
        let pcm: Vec<f32> = (0..FRAME).map(|i| ((i as f32) * 0.1).sin() * 0.3).collect();
        let packet = enc.encode(&pcm).unwrap();
        assert_eq!(dec.decode(&packet).unwrap().len(), FRAME);
        let mut text = String::new();
        append_text(&mut text, &"中".repeat(MAX_TEXT));
        assert!(text.len() <= MAX_TEXT);
        assert!(text.is_char_boundary(text.len()));
    }
}

#[cfg(test)]
mod live_probe {
    use super::*;
    /// Explicit manual diagnostic only. Never runs in ordinary test suites.
    /// HEX_ASSISTANT_PROBE_IDENTITY points to the original assistant identity;
    /// HEX_ASSISTANT_PROBE_WAV points to a benign 16 kHz mono PCM16 WAV.
    #[test]
    #[ignore = "contacts the official assistant and may execute speech instructions"]
    fn official_assistant_probe() {
        let identity =
            std::env::var_os("HEX_ASSISTANT_PROBE_IDENTITY").expect("set identity path explicitly");
        let wav = std::env::var_os("HEX_ASSISTANT_PROBE_WAV").expect("set WAV path explicitly");
        let file = fs::File::open(identity).expect("identity unavailable");
        let mut bytes = Vec::new();
        file.take(32_769)
            .read_to_end(&mut bytes)
            .expect("identity read failed");
        let mut reader = hound::WavReader::open(wav).expect("WAV unavailable");
        let spec = reader.spec();
        assert!(
            spec.sample_rate == 16000
                && spec.channels == 1
                && spec.bits_per_sample == 16
                && spec.sample_format == hound::SampleFormat::Int
        );
        assert!(reader.duration() as usize <= MAX_SAMPLES);
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.expect("invalid PCM") as f32 / 32768.0)
            .collect();
        assert!(samples.len() >= 1600);
        let started = Instant::now();
        let (mut ws, sid, rate) = connect_identity(&bytes).expect("connection failed");
        eprintln!("assistant probe: connected after {:?}", started.elapsed());
        let generation = GENERATION.load(Ordering::SeqCst);
        transmit(
            &mut ws,
            &sid,
            &samples,
            generation,
            &mut Liveness::new(),
            &mut VecDeque::new(),
        )
        .expect("send failed");
        eprintln!("assistant probe: audio sent after {:?}", started.elapsed());
        let mut decoder = Decoder::new(rate).expect("decoder failed");
        let mut saw_stt = false;
        let mut saw_text = false;
        let mut saw_audio = false;
        let mut saw_stop = false;
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v["type"] == "stt" && v["text"].as_str().is_some_and(|s| !s.is_empty()) {
                            saw_stt = true;
                        }
                        if v["type"] == "tts"
                            && v["state"] == "sentence_start"
                            && v["text"].as_str().is_some_and(|s| !s.is_empty())
                        {
                            saw_text = true;
                        }
                        if v["type"] == "tts" && v["state"] == "stop" {
                            saw_stop = true;
                            break;
                        }
                    }
                }
                Ok(Message::Binary(b)) => {
                    if decoder.decode(&b).is_ok() {
                        saw_audio = true;
                    }
                }
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    ()
                }
                Err(_) => break,
                _ => (),
            }
        }
        eprintln!(
            "assistant probe: elapsed {:?}, stt={saw_stt}, reply_text={saw_text}, decoded_audio={saw_audio}, stop={saw_stop}",
            started.elapsed()
        );
        assert!(
            saw_stt && saw_text && saw_audio && saw_stop,
            "assistant response incomplete"
        );
    }
}

//! ESP32-P4 bench voice console — milestone 1: join Wi-Fi (via the onboard C6) and
//! prove a relay round-trip. Audio (ES8311 I2S) and the 720x720 touch UI come later.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::esp_crt_bundle_attach;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::ws::client::{
    EspWebSocketClient, EspWebSocketClientConfig, FrameType, WebSocketEvent, WebSocketEventType,
};

const WIFI_SSID: &str = match option_env!("VOICE_WIFI_SSID") {
    Some(v) => v,
    None => "",
};
const WIFI_PASS: &str = match option_env!("VOICE_WIFI_PASS") {
    Some(v) => v,
    None => "",
};
const DEVICE_ID: &str = match option_env!("VOICE_DEVICE_ID") {
    Some(v) => v,
    None => "VOICE-DEV",
};
const RELAY_BASE: &str = match option_env!("VOICE_RELAY_URL") {
    Some(v) => v,
    None => "wss://socket.master-tech.app/websocket",
};

const SAMPLE_RATE: u32 = 16000;

extern "C" {
    fn audio_init() -> i32;
    fn audio_write(buf: *const u8, len: usize) -> i32;
    fn audio_read(buf: *mut u8, len: usize) -> i32;
    fn audio_set_amp(on: i32);
}

/// Plays a 440 Hz tone for `ms` to the speaker.
fn play_tone(ms: u32) {
    let n = (SAMPLE_RATE * ms / 1000) as usize;
    let mut pcm = Vec::<u8>::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 9000.0;
        pcm.extend_from_slice(&(s as i16).to_le_bytes());
    }
    unsafe {
        audio_set_amp(1);
        audio_write(pcm.as_ptr(), pcm.len());
    }
}

/// Mic→speaker loopback until `run` clears.
fn echo_loop(run: Arc<AtomicBool>) {
    let mut buf = [0u8; 2048];
    unsafe { audio_set_amp(1) };
    while run.load(Ordering::Relaxed) {
        let n = unsafe { audio_read(buf.as_mut_ptr(), buf.len()) };
        if n > 0 {
            unsafe { audio_write(buf.as_ptr(), n as usize) };
        }
    }
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log::info!("voice-console {} starting; device_id={DEVICE_ID}", env!("CARGO_PKG_VERSION"));

    if WIFI_SSID.is_empty() {
        log::error!("no Wi-Fi configured; build with VOICE_WIFI_SSID and VOICE_WIFI_PASS set");
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    let _wifi = connect_wifi()?;
    log::info!("joining relay room {DEVICE_ID}");

    let (tx, rx) = sync_channel::<String>(16);
    let uri = format!("{RELAY_BASE}?room_id={DEVICE_ID}&role=client");
    let config = EspWebSocketClientConfig {
        crt_bundle_attach: Some(esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut client = EspWebSocketClient::new(&uri, &config, Duration::from_secs(10), move |event| {
        on_ws_event(event, &tx)
    })?;

    match unsafe { audio_init() } {
        0 => log::info!("audio ready (es8311, 16 kHz)"),
        e => log::warn!("audio_init failed: {e}"),
    }
    let echo = Arc::new(AtomicBool::new(false));

    loop {
        let Ok(line) = rx.recv() else {
            log::error!("event channel closed; exiting");
            return Ok(());
        };
        for part in line.split('\n').filter(|s| !s.trim().is_empty()) {
            if !part.trim_start().starts_with('{') {
                log::info!("relay control: {}", part.trim());
                continue;
            }
            let reply = handle_command(part, &echo);
            if let Err(e) = client.send(FrameType::Text(false), reply.as_bytes()) {
                log::warn!("relay send failed: {e}");
            }
        }
    }
}

/// Runs a JSON command and returns the reply line.
fn handle_command(cmd: &str, echo: &Arc<AtomicBool>) -> String {
    if cmd.contains("\"beep\"") {
        play_tone(400);
        r#"{"ok":true,"result":{"beeped":true}}"#.to_string()
    } else if cmd.contains("echo_on") {
        if !echo.swap(true, Ordering::Relaxed) {
            let run = echo.clone();
            let _ = std::thread::Builder::new().stack_size(16384).spawn(move || echo_loop(run));
        }
        r#"{"ok":true,"result":{"echo":true}}"#.to_string()
    } else if cmd.contains("echo_off") {
        echo.store(false, Ordering::Relaxed);
        r#"{"ok":true,"result":{"echo":false}}"#.to_string()
    } else {
        format!(
            r#"{{"ok":true,"result":{{"role":"voice-console","firmware":"{}"}}}}"#,
            env!("CARGO_PKG_VERSION")
        )
    }
}

fn connect_wifi() -> Result<BlockingWifi<EspWifi<'static>>> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;
    let auth = if WIFI_PASS.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: WIFI_SSID.try_into().map_err(|_| anyhow!("ssid too long"))?,
        password: WIFI_PASS.try_into().map_err(|_| anyhow!("password too long"))?,
        auth_method: auth,
        ..Default::default()
    }))?;
    wifi.start()?;
    let mut delay = Duration::from_secs(2);
    loop {
        match wifi.connect().and_then(|_| wifi.wait_netif_up()) {
            Ok(()) => break,
            Err(e) => {
                log::warn!("wifi connect to {WIFI_SSID} failed: {e}; retrying in {delay:?}");
                let _ = wifi.disconnect();
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    }
    match wifi.wifi().sta_netif().get_ip_info() {
        Ok(info) => log::info!("wifi up on {WIFI_SSID}, ip {}", info.ip),
        Err(e) => log::warn!("wifi up on {WIFI_SSID}, ip unknown: {e}"),
    }
    Ok(wifi)
}

fn on_ws_event(event: &Result<WebSocketEvent<'_>, EspIOError>, tx: &SyncSender<String>) {
    let Ok(event) = event else { return };
    match &event.event_type {
        WebSocketEventType::Connected => log::info!("relay connected"),
        WebSocketEventType::Disconnected => log::warn!("relay disconnected"),
        WebSocketEventType::Text(t) => {
            let _ = tx.try_send((*t).to_string());
        }
        WebSocketEventType::Binary(b) => {
            if let Ok(s) = std::str::from_utf8(b) {
                let _ = tx.try_send(s.to_string());
            }
        }
        WebSocketEventType::Close(_) | WebSocketEventType::Closed => log::warn!("relay closed"),
        _ => {}
    }
}

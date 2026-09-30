//! Boot, Wi-Fi, the relay socket, and the mic and visualizer threads.

use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::reset::ResetReason;
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::esp_crt_bundle_attach;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::ws::client::{
    EspWebSocketClient, EspWebSocketClientConfig, WebSocketEvent, WebSocketEventType,
};

use crate::console::{self, Console, Mic, Outgoing};
use crate::ffi::{self, color};
use crate::settings::Settings;
use crate::viz;

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

/// Prefixes of voice-bridge's playback control frames.
const TTS_START: &str = r#"{"cmd":"tts_start""#;
const TTS_END: &str = r#"{"cmd":"tts_end""#;
const VIZ_FRAME: Duration = Duration::from_millis(40);
/// Associations without a DHCP lease before the chip restarts.
const NO_LEASE_RESTART: u32 = 2;
const WIFI_CHECK: Duration = Duration::from_secs(5);

pub fn run() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log::info!(
        "voice-console {} starting; device_id={DEVICE_ID}, reset reason {:?}",
        env!("CARGO_PKG_VERSION"),
        ResetReason::get()
    );

    match ffi::init_display() {
        Ok(()) => {
            log::info!("display ready (st7703 720x720)");
            match ffi::start_ui() {
                Ok(()) => log::info!("ui started (lvgl)"),
                Err(e) => log::warn!("ui_start failed: {e}"),
            }
        }
        Err(e) => log::warn!("display_init failed: {e}"),
    }

    let nvs = EspDefaultNvsPartition::take()?;
    let settings = Settings::open(nvs.clone())
        .map_err(|e| log::warn!("settings unavailable: {e}"))
        .ok();

    match ffi::init_audio() {
        Ok(()) => log::info!("audio ready (es8311 out, es7210 dual mic, 16 kHz)"),
        Err(e) => log::warn!("audio_init failed: {e}"),
    }
    let wake = if !settings.as_ref().map_or(true, Settings::wake) {
        log::info!("wake word turned off in settings; push-to-talk only");
        false
    } else {
        match ffi::init_wake() {
            Ok(()) => {
                log::info!("wake word ready (esp-sr afe)");
                true
            }
            Err(e) => {
                log::warn!("wake word unavailable ({e}); push-to-talk only");
                false
            }
        }
    };
    match ffi::attach_touch() {
        Ok(()) => log::info!("touch ready (gt911)"),
        Err(e) => log::warn!("ui_attach_touch failed: {e}"),
    }
    std::thread::Builder::new()
        .name("viz".into())
        .stack_size(12 * 1024)
        .spawn(viz_loop)?;

    if WIFI_SSID.is_empty() {
        log::error!("no Wi-Fi configured; build with VOICE_WIFI_SSID and VOICE_WIFI_PASS set");
        ffi::set_status("No Wi-Fi configured", color::ERROR);
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    ffi::set_status("Joining Wi-Fi...", color::MUTED);
    let wifi = connect_wifi(nvs)?;
    std::thread::Builder::new()
        .name("wifi".into())
        .stack_size(5 * 1024)
        .spawn(move || keep_wifi(wifi))?;
    ffi::set_status("Connecting...", color::MUTED);
    log::info!("joining relay room {DEVICE_ID}");

    let mic = Arc::new(Mic::default());
    let (inbox_tx, inbox_rx) = sync_channel::<String>(16);
    let uri = format!("{RELAY_BASE}?room_id={DEVICE_ID}&role=client");
    let config = EspWebSocketClientConfig {
        crt_bundle_attach: Some(esp_crt_bundle_attach),
        buffer_size: 2048,
        reconnect_timeout_ms: Duration::from_secs(3),
        ..Default::default()
    };
    let cb_mic = mic.clone();
    let client = EspWebSocketClient::new(&uri, &config, Duration::from_secs(10), move |event| {
        on_ws_event(event, &inbox_tx, &cb_mic)
    })?;
    console::log_heap();

    let (out_tx, out_rx) = sync_channel::<Outgoing>(8);
    let loop_mic = mic.clone();
    std::thread::Builder::new()
        .name("mic".into())
        .stack_size(4096)
        .spawn(move || console::mic_loop(loop_mic, out_tx))?;

    Console::new(client, inbox_rx, out_rx, mic, settings, wake).run()
}

/// Feeds the per-mic waveform and spectrum panels.
fn viz_loop() {
    let (mut mic1, mut mic2) = ([0i16; viz::WINDOW], [0i16; viz::WINDOW]);
    let (mut bars1, mut bars2) = (viz::Bars::default(), viz::Bars::default());
    loop {
        ffi::tap_latest(&mut mic1, &mut mic2);
        ffi::viz_update(
            &viz::waveform(&mic1),
            &viz::waveform(&mic2),
            &bars1.update(&viz::bands(&mic1)),
            &bars2.update(&viz::bands(&mic2)),
        );
        std::thread::sleep(VIZ_FRAME);
    }
}

fn connect_wifi(nvs: EspDefaultNvsPartition) -> Result<BlockingWifi<EspWifi<'static>>> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
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
    let mut no_lease = 0;
    loop {
        match wifi.connect() {
            Ok(()) => match wifi.wait_netif_up() {
                Ok(()) => break,
                Err(e) => {
                    no_lease += 1;
                    if no_lease >= NO_LEASE_RESTART {
                        log::error!("associated {no_lease} times without a DHCP lease; restarting");
                        esp_idf_svc::hal::reset::restart();
                    }
                    log::warn!("no DHCP lease on {WIFI_SSID}: {e}; retrying in {delay:?}");
                }
            },
            Err(e) => log::warn!("wifi connect to {WIFI_SSID} failed: {e}; retrying in {delay:?}"),
        }
        let _ = wifi.disconnect();
        std::thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_secs(60));
    }
    match wifi.wifi().sta_netif().get_ip_info() {
        Ok(info) => log::info!("wifi up on {WIFI_SSID}, ip {}", info.ip),
        Err(e) => log::warn!("wifi up on {WIFI_SSID}, ip unknown: {e}"),
    }
    Ok(wifi)
}

/// Rejoins the access point whenever the station loses its link or address.
fn keep_wifi(mut wifi: BlockingWifi<EspWifi<'static>>) {
    loop {
        std::thread::sleep(WIFI_CHECK);
        if wifi.is_up().unwrap_or(false) {
            continue;
        }
        log::warn!("wifi down; rejoining {WIFI_SSID}");
        let _ = wifi.disconnect();
        match wifi.connect().and_then(|()| wifi.wait_netif_up()) {
            Ok(()) => log::info!("wifi back up on {WIFI_SSID}"),
            Err(e) => log::warn!("wifi rejoin failed: {e}"),
        }
    }
}

fn on_ws_event(
    event: &Result<WebSocketEvent<'_>, EspIOError>,
    inbox: &SyncSender<String>,
    mic: &Mic,
) {
    let Ok(event) = event else { return };
    match &event.event_type {
        WebSocketEventType::Connected => {
            log::info!("relay connected");
            let _ = inbox.try_send(console::WS_CONNECTED.to_string());
        }
        WebSocketEventType::Disconnected => {
            log::warn!("relay disconnected");
            let _ = inbox.try_send(console::WS_DISCONNECTED.to_string());
        }
        WebSocketEventType::Text(t) => {
            // Starts and ends playback in callback order with the PCM frames.
            if t.starts_with(TTS_START) && !mic.capturing() {
                ffi::play_begin();
            } else if t.starts_with(TTS_END) {
                ffi::play_end();
            }
            let _ = inbox.try_send((*t).to_string());
        }
        WebSocketEventType::Binary(pcm) => ffi::play_push(pcm),
        WebSocketEventType::Close(_) | WebSocketEventType::Closed => {
            log::warn!("relay closed");
            let _ = inbox.try_send(console::WS_DISCONNECTED.to_string());
        }
        _ => {}
    }
}

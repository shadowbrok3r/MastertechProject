//! Corrects a stale system clock at startup via SNTP, so TLS handshakes don't
//! reject valid certificates as "not valid yet" (the Windows PE / HBCD failure).

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::Duration;

const NTP_SERVERS: &[&str] = &[
    "time.windows.com:123",
    "time.nist.gov:123",
    "pool.ntp.org:123",
];
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;
const SKEW_THRESHOLD_SECS: i64 = 120;
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

fn query_sntp(server: &str, timeout: Duration) -> anyhow::Result<i64> {
    let addr = server
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("no address for {server}"))?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    // 48-byte SNTP request; first byte = LI 0 | VN 4 | Mode 3 (client).
    let mut request = [0u8; 48];
    request[0] = 0x1B;
    socket.send_to(&request, addr)?;

    let mut reply = [0u8; 48];
    let n = socket.recv(&mut reply)?;
    if n < 48 {
        anyhow::bail!("short SNTP reply ({n} bytes) from {server}");
    }
    let ntp_secs = u32::from_be_bytes([reply[40], reply[41], reply[42], reply[43]]) as u64;
    if ntp_secs == 0 {
        anyhow::bail!("SNTP reply from {server} has zero transmit timestamp");
    }
    Ok((ntp_secs - NTP_UNIX_OFFSET) as i64)
}

fn fetch_true_unix_time() -> anyhow::Result<i64> {
    let mut last_err = None;
    for server in NTP_SERVERS {
        match query_sntp(server, QUERY_TIMEOUT) {
            Ok(secs) => {
                log::info!("clock_sync: {server} -> unix {secs}");
                return Ok(secs);
            }
            Err(e) => {
                log::debug!("clock_sync: {server} failed: {e}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no NTP servers configured")))
}

/// Queries public NTP servers and, when the local clock is off by more than two
/// minutes, sets the system clock (UTC). No-op on a healthy clock; returns Err
/// if no server answered, in which case the caller continues unchanged.
pub fn ensure_system_clock_sane() -> anyhow::Result<()> {
    let true_unix = fetch_true_unix_time()?;
    let local_unix = chrono::Utc::now().timestamp();
    let skew = true_unix - local_unix;

    if skew.abs() <= SKEW_THRESHOLD_SECS {
        log::info!("clock_sync: system clock OK (skew {skew}s)");
        return Ok(());
    }

    log::warn!(
        "clock_sync: system clock off by {skew}s (local {local_unix}, true {true_unix}); correcting"
    );
    set_system_clock_utc(true_unix)
}

#[cfg(target_os = "windows")]
fn set_system_clock_utc(unix_secs: i64) -> anyhow::Result<()> {
    use chrono::{Datelike, Timelike};
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::SetSystemTime;

    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(unix_secs, 0)
        .ok_or_else(|| anyhow::anyhow!("unix {unix_secs} out of range"))?;

    let st = SYSTEMTIME {
        wYear: dt.year() as u16,
        wMonth: dt.month() as u16,
        wDayOfWeek: dt.weekday().num_days_from_sunday() as u16,
        wDay: dt.day() as u16,
        wHour: dt.hour() as u16,
        wMinute: dt.minute() as u16,
        wSecond: dt.second() as u16,
        wMilliseconds: 0,
    };
    unsafe { SetSystemTime(&st) }.map_err(|e| anyhow::anyhow!("SetSystemTime failed: {e}"))?;
    log::info!("clock_sync: system clock set to {dt} UTC");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_system_clock_utc(_unix_secs: i64) -> anyhow::Result<()> {
    log::warn!("clock_sync: clock correction is only implemented on Windows");
    Ok(())
}

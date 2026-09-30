//! Injection state and command dispatch, routing to a [`Hid`] backend.

use std::time::{Duration, Instant};

use crate::payload::PayloadStore;
use crate::protocol::{Button, Envelope, Request, Response, Step};
use serde_json::json;

const DEFAULT_ARM_TTL: Duration = Duration::from_secs(120);

/// Emits USB HID reports. The device backend is TinyUSB; tests use a mock.
pub trait Hid {
    fn type_text(&mut self, text: &str) -> anyhow::Result<()>;
    fn key(&mut self, chord: &str) -> anyhow::Result<()>;
    fn mouse_move(&mut self, x: i32, y: i32) -> anyhow::Result<()>;
    fn click(&mut self, button: Button) -> anyhow::Result<()>;
    fn release_all(&mut self);
    fn delay(&mut self, ms: u32);
    fn ready(&self) -> bool;
    /// Drains up to `max` bytes captured on the USB CDC serial endpoint.
    fn read_serial(&mut self, _max: usize) -> Vec<u8> {
        Vec::new()
    }
    /// USB functions this backend exposes, reported by `status`.
    fn capabilities(&self) -> Vec<&'static str> {
        vec!["hid.keyboard", "hid.mouse", "payload"]
    }
}

/// Arms injection and forwards commands to the backend. Boots disarmed.
pub struct Injector<H: Hid> {
    armed_until: Option<Instant>,
    hid: H,
    store: PayloadStore,
}

impl<H: Hid> Injector<H> {
    pub fn new(hid: H) -> Self {
        Self { armed_until: None, hid, store: PayloadStore::new() }
    }

    pub fn armed(&self) -> bool {
        self.armed_until.is_some_and(|t| Instant::now() < t)
    }

    fn arm_remaining_secs(&self) -> u64 {
        self.armed_until
            .and_then(|t| t.checked_duration_since(Instant::now()))
            .map_or(0, |d| d.as_secs())
    }

    /// Arms injection for `ttl` seconds (default 120), refreshing the lease.
    pub fn arm(&mut self, ttl_secs: Option<u64>) {
        let ttl = ttl_secs.map_or(DEFAULT_ARM_TTL, Duration::from_secs);
        self.armed_until = Some(Instant::now() + ttl);
    }

    /// Arms with the default lease, or disarms and drops any held reports.
    pub fn set_armed(&mut self, armed: bool) {
        if armed {
            self.arm(None);
        } else {
            self.armed_until = None;
            self.hid.release_all();
        }
    }

    fn guard(&self) -> anyhow::Result<()> {
        if !self.armed() {
            anyhow::bail!("injection disarmed or arm lease expired; send {{\"cmd\":\"arm\"}} first");
        }
        if !self.hid.ready() {
            anyhow::bail!("usb host not connected");
        }
        Ok(())
    }

    fn status(&self) -> serde_json::Value {
        json!({
            "armed": self.armed(),
            "arm_expires_in_secs": self.arm_remaining_secs(),
            "usb": if self.hid.ready() { "ready" } else { "not_connected" },
            "firmware": env!("CARGO_PKG_VERSION"),
            "capabilities": self.hid.capabilities(),
            "payloads": self.store.list(),
        })
    }

    fn run_steps(&mut self, steps: Vec<Step>) -> anyhow::Result<()> {
        for step in steps {
            self.run_step(step)?;
        }
        Ok(())
    }

    fn run_step(&mut self, step: Step) -> anyhow::Result<()> {
        match step {
            Step::Type { text } => self.hid.type_text(&text),
            Step::Key { chord } => self.hid.key(&chord),
            Step::MouseMove { x, y } => self.hid.mouse_move(x, y),
            Step::Click { button } => self.hid.click(button),
            Step::Delay { ms } => {
                self.hid.delay(ms);
                Ok(())
            }
            Step::ReleaseAll => {
                self.hid.release_all();
                Ok(())
            }
        }
    }

    /// Runs one command and builds its reply.
    pub fn dispatch(&mut self, env: Envelope) -> Response {
        let id = env.id;
        match env.request {
            Request::Ping => Response::ok(id, json!({ "pong": true, "armed": self.armed() })),
            Request::Status => Response::ok(id, self.status()),
            Request::Arm { ttl_secs } => {
                self.arm(ttl_secs);
                Response::ok(id, json!({ "armed": true, "arm_expires_in_secs": self.arm_remaining_secs() }))
            }
            Request::Disarm => {
                self.set_armed(false);
                Response::ok(id, json!({ "armed": false }))
            }
            Request::ReleaseAll => {
                self.hid.release_all();
                Response::ok(id, json!({ "released": true }))
            }
            Request::Type { text } => self.gated(id, |s| s.hid.type_text(&text)),
            Request::Key { chord } => self.gated(id, |s| s.hid.key(&chord)),
            Request::MouseMove { x, y } => self.gated(id, |s| s.hid.mouse_move(x, y)),
            Request::Click { button } => self.gated(id, |s| s.hid.click(button)),
            Request::Combo { steps } => self.gated(id, |s| s.run_steps(steps)),
            Request::PayloadStore { name, steps } => match self.store.put(name, steps) {
                Ok(()) => Response::ok(id, json!({ "stored": true, "payloads": self.store.list() })),
                Err(e) => Response::err(id, e),
            },
            Request::PayloadList => Response::ok(id, json!({ "payloads": self.store.list() })),
            Request::PayloadDelete { name } => {
                let removed = self.store.delete(&name);
                Response::ok(id, json!({ "deleted": removed, "payloads": self.store.list() }))
            }
            Request::PayloadRun { name } => match self.store.steps(&name) {
                Some(steps) => self.gated(id, |s| s.run_steps(steps)),
                None => Response::err(id, format!("no payload named '{name}'")),
            },
            Request::ReadSerial { max_bytes } => {
                let bytes = self.hid.read_serial(max_bytes.unwrap_or(4096));
                let text = String::from_utf8_lossy(&bytes).into_owned();
                Response::ok(id, json!({ "len": bytes.len(), "text": text }))
            }
        }
    }

    fn gated<F>(&mut self, id: Option<String>, f: F) -> Response
    where
        F: FnOnce(&mut Self) -> anyhow::Result<()>,
    {
        if let Err(e) = self.guard() {
            return Response::err(id, e.to_string());
        }
        match f(self) {
            Ok(()) => Response::ok(id, json!({ "done": true })),
            Err(e) => Response::err(id, e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockHid {
        ready: bool,
        log: Vec<String>,
        serial: Vec<u8>,
    }

    impl Hid for MockHid {
        fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
            self.log.push(format!("type:{text}"));
            Ok(())
        }
        fn key(&mut self, chord: &str) -> anyhow::Result<()> {
            self.log.push(format!("key:{chord}"));
            Ok(())
        }
        fn mouse_move(&mut self, x: i32, y: i32) -> anyhow::Result<()> {
            self.log.push(format!("move:{x},{y}"));
            Ok(())
        }
        fn click(&mut self, button: Button) -> anyhow::Result<()> {
            self.log.push(format!("click:{button:?}"));
            Ok(())
        }
        fn release_all(&mut self) {
            self.log.push("release".into());
        }
        fn delay(&mut self, ms: u32) {
            self.log.push(format!("delay:{ms}"));
        }
        fn ready(&self) -> bool {
            self.ready
        }
        fn read_serial(&mut self, max: usize) -> Vec<u8> {
            let n = self.serial.len().min(max);
            self.serial.drain(..n).collect()
        }
    }

    fn env(line: &str) -> Envelope {
        serde_json::from_str(line).expect("parses")
    }

    fn armed_ready() -> Injector<MockHid> {
        let mut inj = Injector::new(MockHid { ready: true, ..Default::default() });
        inj.set_armed(true);
        inj
    }

    #[test]
    fn boots_disarmed() {
        assert!(!Injector::new(MockHid::default()).armed());
    }

    #[test]
    fn injection_blocked_until_armed() {
        let mut inj = Injector::new(MockHid { ready: true, ..Default::default() });
        let r = inj.dispatch(env(r#"{"cmd":"type","text":"hi"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("disarmed"));
    }

    #[test]
    fn armed_but_no_usb_host_is_refused() {
        let mut inj = Injector::new(MockHid { ready: false, ..Default::default() });
        inj.set_armed(true);
        let r = inj.dispatch(env(r#"{"cmd":"type","text":"hi"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("usb host"));
    }

    #[test]
    fn armed_type_and_key_reach_the_backend() {
        let mut inj = armed_ready();
        assert!(inj.dispatch(env(r#"{"cmd":"type","text":"hello"}"#)).ok);
        assert!(inj.dispatch(env(r#"{"cmd":"key","chord":"ctrl+alt+del"}"#)).ok);
        assert_eq!(inj.hid.log, vec!["type:hello", "key:ctrl+alt+del"]);
    }

    #[test]
    fn combo_runs_steps_in_order() {
        let mut inj = armed_ready();
        let r = inj.dispatch(env(
            r#"{"cmd":"combo","steps":[{"op":"key","chord":"F2"},{"op":"delay","ms":0},{"op":"type","text":"BIOS"},{"op":"release_all"}]}"#,
        ));
        assert!(r.ok);
        assert_eq!(inj.hid.log, vec!["key:F2", "delay:0", "type:BIOS", "release"]);
    }

    #[test]
    fn disarm_releases_and_blocks() {
        let mut inj = armed_ready();
        inj.dispatch(env(r#"{"cmd":"disarm"}"#));
        assert!(!inj.armed());
        assert_eq!(inj.hid.log, vec!["release"]);
        assert!(!inj.dispatch(env(r#"{"cmd":"click"}"#)).ok);
    }

    #[test]
    fn release_all_runs_while_disarmed() {
        let mut inj = Injector::new(MockHid { ready: true, ..Default::default() });
        assert!(inj.dispatch(env(r#"{"cmd":"release_all"}"#)).ok);
        assert_eq!(inj.hid.log, vec!["release"]);
    }

    #[test]
    fn status_and_ping_report_state() {
        let inj = armed_ready();
        let mut inj = inj;
        let r = inj.dispatch(env(r#"{"cmd":"status"}"#));
        let v = r.result.unwrap();
        assert_eq!(v["armed"], true);
        assert_eq!(v["usb"], "ready");
        assert!(v["capabilities"].as_array().unwrap().iter().any(|c| c == "payload"));
    }

    #[test]
    fn arm_survives_a_master_disconnect_but_expires_with_the_lease() {
        let mut inj = armed_ready();
        assert!(inj.armed());
        inj.armed_until = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(!inj.armed());
        let r = inj.dispatch(env(r#"{"cmd":"type","text":"x"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("lease expired"));
    }

    #[test]
    fn arm_ttl_is_honored() {
        let mut inj = Injector::new(MockHid { ready: true, ..Default::default() });
        let r = inj.dispatch(env(r#"{"cmd":"arm","ttl_secs":30}"#));
        assert!(r.ok);
        let remaining = r.result.unwrap()["arm_expires_in_secs"].as_u64().unwrap();
        assert!(remaining <= 30 && remaining >= 28);
    }

    #[test]
    fn payload_store_list_run_delete() {
        let mut inj = armed_ready();
        let stored = inj.dispatch(env(
            r#"{"cmd":"payload_store","name":"bios","steps":[{"op":"key","chord":"F2"},{"op":"type","text":"x"}]}"#,
        ));
        assert!(stored.ok);
        let list = inj.dispatch(env(r#"{"cmd":"payload_list"}"#));
        assert_eq!(list.result.unwrap()["payloads"][0]["name"], "bios");

        assert!(inj.dispatch(env(r#"{"cmd":"payload_run","name":"bios"}"#)).ok);
        assert_eq!(inj.hid.log, vec!["key:F2", "type:x"]);

        let del = inj.dispatch(env(r#"{"cmd":"payload_delete","name":"bios"}"#));
        assert_eq!(del.result.unwrap()["deleted"], true);
        assert!(!inj.dispatch(env(r#"{"cmd":"payload_run","name":"bios"}"#)).ok);
    }

    #[test]
    fn payload_stores_while_disarmed_but_runs_only_armed() {
        let mut inj = Injector::new(MockHid { ready: true, ..Default::default() });
        assert!(inj.dispatch(env(r#"{"cmd":"payload_store","name":"p","steps":[{"op":"type","text":"a"}]}"#)).ok);
        let r = inj.dispatch(env(r#"{"cmd":"payload_run","name":"p"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("disarmed"));
    }

    #[test]
    fn read_serial_drains_captured_bytes() {
        let mut inj = Injector::new(MockHid { ready: true, serial: b"log line\n".to_vec(), ..Default::default() });
        let r = inj.dispatch(env(r#"{"cmd":"read_serial"}"#));
        let v = r.result.unwrap();
        assert_eq!(v["len"], 9);
        assert_eq!(v["text"], "log line\n");
        assert_eq!(inj.dispatch(env(r#"{"cmd":"read_serial"}"#)).result.unwrap()["len"], 0);
    }
}

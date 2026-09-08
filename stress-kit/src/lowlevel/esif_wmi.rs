//! Intel DPTF/ESIF temperatures over WMI.
//!
//! The Intel Dynamic Tuning stack publishes `EsifDeviceInformation` in
//! `root\wmi`, one instance per participant domain, each carrying a whole-degree
//! Celsius `Temperature` and the participant's trip points. It needs no kernel
//! driver and no elevation, so it reads on machines where WinRing0 will not load
//! at all — the common case on Intel laptops.
//!
//! Only the processor participant feeds a CPU temperature. Its instances are the
//! PCI system-agent device (`PCI\VEN_8086&DEV_…`) or the classic `ACPI\INT3401`
//! node; every other participant is a skin, charger, or memory sensor and is
//! ignored rather than passed off as a CPU reading.
//!
//! `WMIConnection` is deliberately `!Send`, so the connection lives on a worker
//! thread this backend owns and reads cross the thread boundary as decoded rows.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use wmi::{Variant, WMIConnection};

use super::{BackendId, Capabilities, DomainTemp, LowLevelBackend, PackageTempAccess};

const NAMESPACE: &str = "root\\wmi";
const QUERY: &str = "SELECT * FROM EsifDeviceInformation";

/// Longest wait for the worker to build its WMI connection.
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest wait for one query, so a wedged WMI service cannot stall a sampler.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// HRESULTs meaning the namespace or class is not on this machine, as they
/// appear in the WMI error text: `WBEM_E_NOT_FOUND`, `WBEM_E_NOT_SUPPORTED`,
/// `WBEM_E_INVALID_NAMESPACE`, `WBEM_E_INVALID_CLASS`.
const ABSENT_HRESULTS: [&str; 4] = ["0X80041002", "0X8004100C", "0X8004100E", "0X80041010"];

/// One `EsifDeviceInformation` instance, decoded off the WMI thread.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EsifRow {
    /// Full instance name: device path plus the `_<domain>` suffix.
    pub instance: String,
    /// `None` when the domain reports the zero sentinel for "does not measure".
    pub temp_c: Option<f32>,
    pub psv_c: Option<f32>,
    pub crt_c: Option<f32>,
    pub hot_c: Option<f32>,
    pub ac0_c: Option<f32>,
}

enum Job {
    Read(Sender<Result<Vec<EsifRow>, String>>),
}

pub struct EsifWmiBackend {
    jobs: Mutex<Sender<Job>>,
    /// Device path of the processor participant, without the domain suffix.
    participant: String,
}

impl EsifWmiBackend {
    /// `Err` carries the operator-actionable reason: no namespace, no class, or
    /// no processor participant behind it.
    pub fn open() -> Result<Self, String> {
        let jobs = spawn_worker()?;
        let mut me = Self { jobs: Mutex::new(jobs), participant: String::new() };

        let rows = me.read_rows()?;
        me.participant = choose_participant(&rows).ok_or_else(|| {
            format!(
                "{NAMESPACE} answers but no Intel processor participant reports a temperature \
                 ({}); the rest are skin and charger sensors, not a CPU sensor",
                participant_summary(&rows)
            )
        })?;
        log_trip_points(&rows, &me.participant);
        Ok(me)
    }

    /// One query on the worker thread. `Err` is a transport failure, never an
    /// empty sensor.
    fn read_rows(&self) -> Result<Vec<EsifRow>, String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .lock()
            .map_err(|_| "ESIF worker channel poisoned".to_string())?
            .send(Job::Read(tx))
            .map_err(|_| "ESIF WMI worker thread has exited".to_string())?;
        match rx.recv_timeout(READ_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("ESIF WMI query did not answer within {READ_TIMEOUT:?}"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err("ESIF WMI worker thread has exited".to_string())
            }
        }
    }
}

/// Starts the worker and waits for it to build its connection.
fn spawn_worker() -> Result<Sender<Job>, String> {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("stress-kit-esif-wmi".into())
        .spawn(move || worker_loop(jobs_rx, ready_tx))
        .map_err(|e| format!("cannot spawn the ESIF WMI worker thread: {e}"))?;
    match ready_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(())) => Ok(jobs_tx),
        Ok(Err(reason)) => Err(reason),
        Err(_) => Err(format!("the WMI service did not answer within {OPEN_TIMEOUT:?}")),
    }
}

/// Owns the COM apartment and the connection for the life of the backend; exits
/// when the last job sender drops.
fn worker_loop(jobs: Receiver<Job>, ready: Sender<Result<(), String>>) {
    let wmi = match WMIConnection::with_namespace_path(NAMESPACE) {
        Ok(w) => w,
        Err(e) => {
            let _ = ready.send(Err(describe_wmi_error(&e.to_string())));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    while let Ok(Job::Read(reply)) = jobs.recv() {
        let _ = reply.send(query_rows(&wmi));
    }
    log::debug!("stress-kit/esif-wmi: worker thread exiting");
}

fn query_rows(wmi: &WMIConnection) -> Result<Vec<EsifRow>, String> {
    let raw: Vec<HashMap<String, Variant>> = wmi
        .raw_query(QUERY)
        .map_err(|e| describe_wmi_error(&e.to_string()))?;
    Ok(raw.iter().filter_map(decode_row).collect())
}

fn decode_row(props: &HashMap<String, Variant>) -> Option<EsifRow> {
    let instance = match props.get("InstanceName") {
        Some(Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => return None,
    };
    Some(EsifRow {
        instance,
        temp_c: measured(number(props.get("Temperature"))),
        psv_c: measured(number(props.get("PsvTripPoint"))),
        crt_c: measured(number(props.get("CrtTripPoint"))),
        hot_c: measured(number(props.get("HotTripPoint"))),
        ac0_c: measured(number(props.get("Ac0TripPoint"))),
    })
}

/// Numeric value of a variant whatever CIM type the provider declared it as; the
/// class is undocumented and its property types vary by ESIF build.
fn number(v: Option<&Variant>) -> Option<f32> {
    match v? {
        Variant::I1(n) => Some(f32::from(*n)),
        Variant::I2(n) => Some(f32::from(*n)),
        Variant::I4(n) => Some(*n as f32),
        Variant::I8(n) => Some(*n as f32),
        Variant::UI1(n) => Some(f32::from(*n)),
        Variant::UI2(n) => Some(f32::from(*n)),
        Variant::UI4(n) => Some(*n as f32),
        Variant::UI8(n) => Some(*n as f32),
        Variant::R4(n) => Some(*n),
        Variant::R8(n) => Some(*n as f32),
        Variant::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// ESIF reports zero for a field this domain does not measure, so zero is
/// absence and never a reading. Plausibility limits stay in `telemetry`.
fn measured(value: Option<f32>) -> Option<f32> {
    value.filter(|v| *v > 0.0)
}

/// Device path and domain index from an instance name. The domain suffix is the
/// trailing `_<n>`; a name without one is domain 0.
pub(crate) fn split_instance(instance: &str) -> (&str, u32) {
    match instance.rsplit_once('_') {
        Some((base, suffix)) => match suffix.parse() {
            Ok(index) => (base, index),
            Err(_) => (instance, 0),
        },
        None => (instance, 0),
    }
}

/// True for the DPTF processor participant: the PCI system-agent device, or the
/// classic `ACPI\INT3401` node on pre-Dynamic-Tuning platforms.
pub(crate) fn is_processor_participant(base: &str) -> bool {
    let upper = base.to_ascii_uppercase();
    upper.starts_with("PCI\\VEN_8086") || upper.starts_with("ACPI\\INT3401")
}

/// Device path of the processor participant with the most measuring domains;
/// `None` when no processor participant reports a temperature at all.
pub(crate) fn choose_participant(rows: &[EsifRow]) -> Option<String> {
    let mut measuring: HashMap<&str, usize> = HashMap::new();
    for row in rows.iter().filter(|r| r.temp_c.is_some()) {
        let (base, _) = split_instance(&row.instance);
        if is_processor_participant(base) {
            *measuring.entry(base).or_default() += 1;
        }
    }
    measuring
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(base, _)| base.to_string())
}

/// Every measuring domain of one participant, lowest index first.
pub(crate) fn domains_for(rows: &[EsifRow], participant: &str) -> Vec<DomainTemp> {
    let mut out: Vec<DomainTemp> = rows
        .iter()
        .filter_map(|row| {
            let (base, index) = split_instance(&row.instance);
            (base == participant).then_some(DomainTemp { index, temp_c: row.temp_c? })
        })
        .collect();
    out.sort_by_key(|d| d.index);
    out
}

/// Participant device paths seen, for the message when none is a processor.
fn participant_summary(rows: &[EsifRow]) -> String {
    let mut seen: Vec<&str> = rows.iter().map(|r| split_instance(&r.instance).0).collect();
    seen.sort_unstable();
    seen.dedup();
    if seen.is_empty() {
        return "no participants".to_string();
    }
    format!("participants: {}", seen.join(", "))
}

/// Records the participant's trip points once. They are not a thermal ceiling:
/// the active-cooling steps are fan thresholds, and Psv/Crt/Hot read zero on
/// platforms that publish no passive policy.
fn log_trip_points(rows: &[EsifRow], participant: &str) {
    let Some(row) = rows.iter().find(|r| {
        let (base, _) = split_instance(&r.instance);
        base == participant && r.temp_c.is_some()
    }) else {
        return;
    };
    log::debug!(
        "stress-kit/esif-wmi: processor participant {participant} — {} measuring domain(s), \
         trips ac0={:?} psv={:?} crt={:?} hot={:?}",
        domains_for(rows, participant).len(),
        row.ac0_c,
        row.psv_c,
        row.crt_c,
        row.hot_c
    );
}

/// Turns a WMI error into a reason an operator can act on, naming absence
/// separately from a live provider that failed.
fn describe_wmi_error(error: &str) -> String {
    if is_absent(error) {
        return format!(
            "Intel Dynamic Tuning (DPTF) is not installed on this machine, so \
             {NAMESPACE}:EsifDeviceInformation does not exist ({error})"
        );
    }
    format!("{NAMESPACE}:EsifDeviceInformation could not be queried: {error}")
}

/// True when the error text carries an HRESULT meaning the namespace or class is
/// not present, as opposed to a provider that exists and failed.
fn is_absent(error: &str) -> bool {
    let upper = error.to_ascii_uppercase();
    ABSENT_HRESULTS.iter().any(|h| upper.contains(h))
}

impl PackageTempAccess for EsifWmiBackend {
    fn read_package_domains(&self) -> Vec<DomainTemp> {
        match self.read_rows() {
            Ok(rows) => domains_for(&rows, &self.participant),
            Err(reason) => {
                log::debug!("stress-kit/esif-wmi: read failed: {reason}");
                Vec::new()
            }
        }
    }
}

impl LowLevelBackend for EsifWmiBackend {
    fn id(&self) -> BackendId {
        BackendId::EsifWmi
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { package_temp: true, ..Default::default() }
    }

    fn probe(&self) -> Result<(), String> {
        self.read_rows().map(|_| ())
    }

    fn package_temp(&self) -> Option<&dyn PackageTempAccess> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(instance: &str, temp_c: Option<f32>) -> EsifRow {
        EsifRow {
            instance: instance.to_string(),
            temp_c,
            psv_c: None,
            crt_c: None,
            hot_c: None,
            ac0_c: None,
        }
    }

    /// The device path carries its own underscores, so only the trailing numeric
    /// suffix is the domain.
    #[test]
    fn the_domain_suffix_is_the_trailing_number() {
        let name = "PCI\\VEN_8086&DEV_1903&SUBSYS_384917AA&REV_0C\\3&11583659&1&20_5";
        let (base, index) = split_instance(name);
        assert_eq!(base, "PCI\\VEN_8086&DEV_1903&SUBSYS_384917AA&REV_0C\\3&11583659&1&20");
        assert_eq!(index, 5);
    }

    #[test]
    fn a_name_with_no_numeric_suffix_is_domain_zero() {
        assert_eq!(split_instance("ACPI\\INT3401\\0"), ("ACPI\\INT3401\\0", 0));
        assert_eq!(split_instance("ACPI\\INT3403\\TMEM"), ("ACPI\\INT3403\\TMEM", 0));
    }

    #[test]
    fn only_the_processor_participant_qualifies() {
        assert!(is_processor_participant("PCI\\VEN_8086&DEV_1903&SUBSYS_384917AA&REV_0C\\3&1&20"));
        assert!(is_processor_participant("ACPI\\INT3401\\1"));
        assert!(!is_processor_participant("ACPI\\INT3403\\TSKN"));
        assert!(!is_processor_participant("ACPI\\INTC1046\\0"));
        assert!(!is_processor_participant("PCI\\VEN_8087&DEV_0AAA\\1"));
    }

    /// A skin sensor hotter than the CPU must never become the CPU reading.
    #[test]
    fn a_generic_participant_is_never_chosen() {
        let rows = vec![
            row("ACPI\\INT3403\\TSKN_0", Some(58.0)),
            row("PCI\\VEN_8086&DEV_1903\\3&1&20_0", Some(40.0)),
        ];
        let picked = choose_participant(&rows).expect("processor participant present");
        assert_eq!(picked, "PCI\\VEN_8086&DEV_1903\\3&1&20");
        assert_eq!(
            domains_for(&rows, &picked),
            vec![DomainTemp { index: 0, temp_c: 40.0 }]
        );
    }

    /// Nothing is passed off as a CPU sensor when only generic participants exist.
    #[test]
    fn no_processor_participant_chooses_nothing() {
        let rows = vec![
            row("ACPI\\INT3403\\TSKN_0", Some(45.0)),
            row("ACPI\\INT3403\\TMEM_0", Some(38.0)),
        ];
        assert_eq!(choose_participant(&rows), None);
    }

    /// A participant whose every domain reads the zero sentinel measures nothing.
    #[test]
    fn a_participant_with_no_measuring_domain_is_not_chosen() {
        let rows = vec![
            row("PCI\\VEN_8086&DEV_1903\\3&1&20_0", None),
            row("PCI\\VEN_8086&DEV_1903\\3&1&20_1", None),
        ];
        assert_eq!(choose_participant(&rows), None);
    }

    /// Domains come back in index order with the sentinel ones dropped, so the
    /// caller sees the whole per-domain set the machine measures.
    #[test]
    fn domains_are_ordered_and_sentinel_free() {
        let base = "PCI\\VEN_8086&DEV_1903\\3&1&20";
        let rows = vec![
            row(&format!("{base}_2"), Some(45.0)),
            row(&format!("{base}_0"), Some(40.0)),
            row(&format!("{base}_1"), None),
            row(&format!("{base}_3"), Some(33.0)),
            row("ACPI\\INT3403\\TSKN_0", Some(58.0)),
        ];
        assert_eq!(
            domains_for(&rows, base),
            vec![
                DomainTemp { index: 0, temp_c: 40.0 },
                DomainTemp { index: 2, temp_c: 45.0 },
                DomainTemp { index: 3, temp_c: 33.0 },
            ]
        );
    }

    #[test]
    fn zero_is_absence_and_a_real_reading_survives() {
        assert_eq!(measured(Some(0.0)), None);
        assert_eq!(measured(Some(-1.0)), None);
        assert_eq!(measured(None), None);
        assert_eq!(measured(Some(45.0)), Some(45.0));
    }

    #[test]
    fn any_numeric_cim_type_decodes() {
        assert_eq!(number(Some(&Variant::UI4(45))), Some(45.0));
        assert_eq!(number(Some(&Variant::I4(45))), Some(45.0));
        assert_eq!(number(Some(&Variant::UI1(45))), Some(45.0));
        assert_eq!(number(Some(&Variant::R8(45.5))), Some(45.5));
        assert_eq!(number(Some(&Variant::String("45".into()))), Some(45.0));
        assert_eq!(number(Some(&Variant::Null)), None);
        assert_eq!(number(None), None);
    }

    /// Reports what this machine's DPTF stack actually publishes. Ignored by
    /// default because the answer is hardware, not a contract:
    /// `cargo test -p stress-kit --features hw-sensors esif -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reports_what_this_machine_publishes() {
        match EsifWmiBackend::open() {
            Ok(backend) => {
                println!("participant: {}", backend.participant);
                for domain in backend.read_package_domains() {
                    println!("  domain {} = {} C", domain.index, domain.temp_c);
                }
                assert!(
                    !backend.read_package_domains().is_empty(),
                    "a participant was chosen but reads nothing"
                );
            }
            Err(reason) => println!("no DPTF package temperature here: {reason}"),
        }
    }

    #[test]
    fn absent_hresults_read_as_absence_and_others_do_not() {
        assert!(is_absent("HRESULT Call failed with: 0x80041010"));
        assert!(is_absent("hresult call failed with: 0x8004100e"));
        assert!(!is_absent("HRESULT Call failed with: 0x80041033"));
        assert!(describe_wmi_error("0x80041010").contains("not installed"));
        assert!(describe_wmi_error("rpc unavailable").contains("could not be queried"));
    }
}

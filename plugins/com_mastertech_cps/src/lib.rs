//! CPS security-software plugin (Webroot + SUPERAntiSpyware).
//!
//! Read-only license/state checks, a corrected Windows Security Center product
//! survey, a deep Webroot remnant survey, and a confirm-gated full Webroot
//! uninstall. Split out of hw-diag so hardware diagnostics and the shop's
//! security bundle are separate concerns. WSC productState is decoded on the
//! middle byte (real-time state), not the low byte (signature state). SAS
//! registration comes from its own SETTINGS databases.

use facet::Facet;
use mtech_plugin_sdk::{SdkError, host, mtech_plugin};
use serde::Deserialize;

#[derive(Facet, Deserialize)]
struct CleanupArgs {
    /// Must be true. Acknowledges this is a FULL Webroot uninstall, not remnant pruning.
    confirm_full_uninstall: Option<bool>,
}

/// Parses the PS JSON output into the tool envelope, stderr-safe.
fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

/// Standard padded base64; whitespace is skipped.
fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err(format!("invalid base64 byte {c:#04x}")),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Read-only reader for the table b-trees of a small SQLite database file.
mod sqlite {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Int(i64),
        Real(f64),
        Text(Vec<u8>),
        Blob(Vec<u8>),
    }

    /// Every row of `table`, columns in declaration order.
    pub fn table_rows(db: &[u8], table: &str) -> Result<Vec<Vec<Value>>, String> {
        let file = File::open(db)?;
        let mut root = None;
        file.walk(1, 0, &mut |row| {
            if let [
                Value::Text(kind),
                Value::Text(name),
                _,
                Value::Int(page),
                ..,
            ] = row.as_slice()
                && kind == b"table"
                && name.eq_ignore_ascii_case(table.as_bytes())
            {
                root = Some(*page);
            }
        })?;
        let root = root.ok_or_else(|| format!("no table named {table}"))?;
        let root = u32::try_from(root).map_err(|_| format!("bad root page {root}"))?;
        let mut rows = Vec::new();
        file.walk(root, 0, &mut |row| rows.push(row))?;
        Ok(rows)
    }

    struct File<'a> {
        db: &'a [u8],
        page_size: usize,
        usable: usize,
    }

    impl<'a> File<'a> {
        fn open(db: &'a [u8]) -> Result<Self, String> {
            if db.len() < 100 || &db[..16] != b"SQLite format 3\0" {
                return Err("not a SQLite database".into());
            }
            let page_size = match u16::from_be_bytes([db[16], db[17]]) {
                1 => 65536,
                n if n >= 512 => usize::from(n),
                n => return Err(format!("bad page size {n}")),
            };
            let usable = page_size - usize::from(db[20]);
            Ok(Self {
                db,
                page_size,
                usable,
            })
        }

        fn page(&self, number: u32) -> Result<&'a [u8], String> {
            let index = usize::try_from(number).map_err(|_| "page number overflow")?;
            let start = index.checked_sub(1).ok_or("page 0 does not exist")? * self.page_size;
            self.db
                .get(start..start + self.page_size)
                .ok_or_else(|| format!("page {number} is past the end of the file"))
        }

        fn walk(
            &self,
            number: u32,
            depth: u32,
            visit: &mut dyn FnMut(Vec<Value>),
        ) -> Result<(), String> {
            if depth > 20 {
                return Err("b-tree deeper than 20 levels".into());
            }
            let page = self.page(number)?;
            let header = if number == 1 { 100 } else { 0 };
            let cells = usize::from(be16(page, header + 3)?);
            match page[header] {
                0x05 => {
                    for i in 0..cells {
                        let cell = usize::from(be16(page, header + 12 + 2 * i)?);
                        self.walk(be32(page, cell)?, depth + 1, visit)?;
                    }
                    self.walk(be32(page, header + 8)?, depth + 1, visit)
                }
                0x0D => {
                    for i in 0..cells {
                        let cell = usize::from(be16(page, header + 8 + 2 * i)?);
                        let (len, a) = varint(page, cell)?;
                        let (_, b) = varint(page, cell + a)?;
                        let len = usize::try_from(len).map_err(|_| "payload too large")?;
                        visit(record(&self.payload(page, cell + a + b, len)?)?);
                    }
                    Ok(())
                }
                kind => Err(format!(
                    "page {number} has b-tree type {kind:#04x}, not a table page"
                )),
            }
        }

        /// A cell's payload, following its overflow chain.
        fn payload(&self, page: &[u8], start: usize, len: usize) -> Result<Vec<u8>, String> {
            let max_local = self.usable - 35;
            if len <= max_local {
                return slice(page, start, len).map(<[u8]>::to_vec);
            }
            let min_local = (self.usable - 12) * 32 / 255 - 23;
            let spill = min_local + (len - min_local) % (self.usable - 4);
            let local = if spill <= max_local { spill } else { min_local };
            let mut out = slice(page, start, local)?.to_vec();
            let mut next = be32(page, start + local)?;
            for _ in 0..self.db.len() / self.page_size {
                if out.len() >= len {
                    break;
                }
                let overflow = self.page(next)?;
                next = be32(overflow, 0)?;
                let take = (len - out.len()).min(self.usable - 4);
                out.extend_from_slice(slice(overflow, 4, take)?);
            }
            if out.len() < len {
                return Err("overflow chain ends early".into());
            }
            Ok(out)
        }
    }

    fn record(payload: &[u8]) -> Result<Vec<Value>, String> {
        let (header_len, mut pos) = varint(payload, 0)?;
        let header_len = usize::try_from(header_len).map_err(|_| "bad record header")?;
        let mut body = header_len;
        let mut values = Vec::new();
        while pos < header_len {
            let (serial, n) = varint(payload, pos)?;
            pos += n;
            let (value, size) = match serial {
                0 => (Value::Null, 0),
                1..=6 => {
                    let size = [0, 1, 2, 3, 4, 6, 8][serial as usize];
                    let bytes = slice(payload, body, size)?;
                    let int = bytes[1..]
                        .iter()
                        .fold(i64::from(bytes[0] as i8), |v, &b| (v << 8) | i64::from(b));
                    (Value::Int(int), size)
                }
                7 => {
                    let bytes: [u8; 8] = slice(payload, body, 8)?
                        .try_into()
                        .map_err(|_| "bad real")?;
                    (Value::Real(f64::from_be_bytes(bytes)), 8)
                }
                8 => (Value::Int(0), 0),
                9 => (Value::Int(1), 0),
                n if n >= 12 => {
                    let size = usize::try_from((n - 12) / 2).map_err(|_| "bad length")?;
                    let bytes = slice(payload, body, size)?.to_vec();
                    (
                        if n % 2 == 0 {
                            Value::Blob(bytes)
                        } else {
                            Value::Text(bytes)
                        },
                        size,
                    )
                }
                n => return Err(format!("reserved serial type {n}")),
            };
            values.push(value);
            body += size;
        }
        Ok(values)
    }

    fn varint(buf: &[u8], pos: usize) -> Result<(u64, usize), String> {
        let mut v = 0u64;
        for i in 0..8 {
            let b = *buf.get(pos + i).ok_or("varint runs past the buffer")?;
            v = (v << 7) | u64::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok((v, i + 1));
            }
        }
        let last = *buf.get(pos + 8).ok_or("varint runs past the buffer")?;
        Ok(((v << 8) | u64::from(last), 9))
    }

    fn slice(buf: &[u8], start: usize, len: usize) -> Result<&[u8], String> {
        let end = start.checked_add(len).ok_or("length overflow")?;
        buf.get(start..end)
            .ok_or_else(|| "read past the end of a page".to_string())
    }

    fn be16(buf: &[u8], pos: usize) -> Result<u16, String> {
        let b = slice(buf, pos, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn be32(buf: &[u8], pos: usize) -> Result<u32, String> {
        let b = slice(buf, pos, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// SUPERAntiSpyware state from its SETTINGS databases.
mod sas {
    use super::sqlite::{self, Value};

    const ANSI: i64 = 256;
    const WIDE: i64 = 257;
    const SYSTEMTIME: i64 = 263;

    pub struct Setting {
        name: String,
        kind: i64,
        data: Vec<u8>,
    }

    /// The SETTINGS rows of one SAS database.
    pub fn settings(db: &[u8]) -> Result<Vec<Setting>, String> {
        Ok(sqlite::table_rows(db, "SETTINGS")?
            .into_iter()
            .filter_map(|row| match row.as_slice() {
                [_, Value::Text(name), Value::Int(kind), data, ..] => Some(Setting {
                    name: String::from_utf8_lossy(name).into_owned(),
                    kind: *kind,
                    data: match data {
                        Value::Blob(b) | Value::Text(b) => b.clone(),
                        _ => Vec::new(),
                    },
                }),
                _ => None,
            })
            .collect())
    }

    fn get<'a>(settings: &'a [Setting], name: &str) -> Option<&'a Setting> {
        settings.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// A string setting, cut at its NUL terminator.
    fn text(settings: &[Setting], name: &str) -> Option<String> {
        let s = get(settings, name)?;
        match s.kind {
            ANSI => {
                let bytes = s.data.split(|&b| b == 0).next().unwrap_or_default();
                Some(String::from_utf8_lossy(bytes).into_owned())
            }
            WIDE => {
                let units: Vec<u16> = s
                    .data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| u16::from_le_bytes(c))
                    .take_while(|&u| u != 0)
                    .collect();
                Some(String::from_utf16_lossy(&units))
            }
            _ => None,
        }
    }

    fn flag(settings: &[Setting], name: &str) -> Option<bool> {
        text(settings, name).map(|v| v.eq_ignore_ascii_case("yes"))
    }

    /// A SYSTEMTIME setting's date; None when unset.
    fn date(settings: &[Setting], name: &str) -> Option<Date> {
        let s = get(settings, name).filter(|s| s.kind == SYSTEMTIME && s.data.len() >= 8)?;
        let word = |i: usize| u16::from_le_bytes([s.data[2 * i], s.data[2 * i + 1]]);
        Date::new(i32::from(word(0)), u32::from(word(1)), u32::from(word(3)))
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Date {
        year: i32,
        month: u32,
        day: u32,
    }

    impl Date {
        pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
            (year > 1900 && (1..=12).contains(&month) && (1..=31).contains(&day)).then_some(Self {
                year,
                month,
                day,
            })
        }

        /// Parses `YYYY-MM-DD`.
        pub fn parse(s: &str) -> Option<Self> {
            let mut parts = s.trim().splitn(3, '-');
            let year = parts.next()?.parse().ok()?;
            let month = parts.next()?.parse().ok()?;
            let day = parts.next()?.get(..2)?.parse().ok()?;
            Self::new(year, month, day)
        }

        /// Days since 1970-01-01.
        fn days(self) -> i64 {
            let y = i64::from(self.year) - i64::from(self.month <= 2);
            let era = y.div_euclid(400);
            let yoe = y - era * 400;
            let m = i64::from(self.month);
            let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
            let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
            era * 146_097 + doe - 719_468
        }

        pub fn days_until(self, later: Date) -> i64 {
            later.days() - self.days()
        }
    }

    impl std::fmt::Display for Date {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
        }
    }

    /// Registration state from SAS_ALLUSER.DB3.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Registration {
        pub registered: bool,
        pub expires: Option<Date>,
        pub expire_to_free: Option<bool>,
    }

    pub fn registration(alluser: &[Setting]) -> Registration {
        Registration {
            registered: text(alluser, "RegCodeEx").is_some_and(|code| !code.trim().is_empty()),
            expires: date(alluser, "SubscriptionExpiration"),
            expire_to_free: flag(alluser, "ExpireToFree"),
        }
    }

    /// Real-time protection and PRO-upgrade flags from one SAS_CURRENTUSER.DB3.
    pub fn user_state(user: &[Setting]) -> (Option<bool>, Option<bool>) {
        (
            flag(user, "EnableRealTimeProtection"),
            flag(user, "UpgradeToProfessionalCompleted"),
        )
    }
}

/// Decodes one base64 SAS database from the `SAS_DB_PS` output.
fn sas_settings(entry: &serde_json::Value) -> Result<Vec<sas::Setting>, String> {
    if let Some(err) = entry.get("error").and_then(serde_json::Value::as_str) {
        return Err(err.to_string());
    }
    let b64 = entry
        .get("b64")
        .and_then(serde_json::Value::as_str)
        .ok_or("no database bytes")?;
    sas::settings(&b64_decode(b64)?)
}

/// Folds the SAS settings databases into the `sas_license` output.
fn merge_sas_db(output: &mut serde_json::Map<String, serde_json::Value>, db: &serde_json::Value) {
    use serde_json::json;
    let Some(alluser) = db.get("alluser").filter(|v| !v.is_null()) else {
        return;
    };
    let settings = match sas_settings(alluser) {
        Ok(settings) => settings,
        Err(e) => {
            output.insert("db_error".into(), json!(e));
            return;
        }
    };
    let reg = sas::registration(&settings);
    let today = db
        .get("today")
        .and_then(serde_json::Value::as_str)
        .and_then(sas::Date::parse);
    let days = reg
        .expires
        .zip(today)
        .map(|(expires, now)| now.days_until(expires));
    let expired = days.map(|d| d < 0);
    let users: Vec<serde_json::Value> = db
        .get("users")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|u| {
            let name = u.get("user").cloned().unwrap_or_default();
            match sas_settings(u).map(|s| sas::user_state(&s)) {
                Ok((real_time, pro)) => {
                    json!({ "user": name, "real_time_on": real_time, "pro_upgrade_completed": pro })
                }
                Err(e) => json!({ "user": name, "error": e }),
            }
        })
        .collect();
    let real_time_on = users
        .iter()
        .filter_map(|u| u["real_time_on"].as_bool())
        .reduce(|a, b| a || b);
    let note = if !reg.registered {
        Some("Unregistered (Free Edition): real-time protection, scheduled scans and automatic updates are PRO-only.".to_string())
    } else if expired == Some(true) {
        reg.expires.map(|d| format!("Subscription expired on {d}."))
    } else if real_time_on == Some(false) {
        Some("Registered, but real-time protection is off.".to_string())
    } else {
        None
    };
    output.insert("installed".into(), json!(true));
    output.insert("registered".into(), json!(reg.registered));
    output.insert(
        "subscription_expiration".into(),
        json!(reg.expires.map(|d| d.to_string())),
    );
    output.insert("days_remaining".into(), json!(days));
    output.insert("expired".into(), json!(expired));
    output.insert("expire_to_free".into(), json!(reg.expire_to_free));
    output.insert("real_time_on".into(), json!(real_time_on));
    output.insert("users".into(), json!(users));
    output.insert(
        "active".into(),
        json!(reg.registered && expired != Some(true) && real_time_on != Some(false)),
    );
    output.insert("source".into(), json!("SAS_ALLUSER.DB3"));
    if alluser.get("wal").and_then(serde_json::Value::as_bool) == Some(true) {
        output.insert("wal_pending".into(), json!(true));
    }
    output.insert("note".into(), json!(note));
}

const WEBROOT_LICENSE_PS: &str = r#"$o=[ordered]@{product='webroot';installed=$false;active=$null;days_remaining=$null;source=$null;executable=$null;wmi=$null;registry_hits=@();note=$null};
$wrExe=@('C:\Program Files\Webroot\WRSA.exe','C:\Program Files (x86)\Webroot\WRSA.exe')|Where-Object{Test-Path $_}|Select-Object -First 1;
if($wrExe){$o.installed=$true;$o.executable=$wrExe};
function Add-Hit($path,$name,$val){
  if($val -is [byte[]]){return};
  $s=[string]$val;
  if($s.Length -gt 120){$s=$s.Substring(0,120)+'…'};
  $script:o.registry_hits+=[ordered]@{path=$path;name=$name;value=$s}
};
function Try-DaysFromNameValue($path,$name,$val){
  if($val -is [byte[]]){return};
  if($name -match '(?i)day'){
    if($val -is [int] -or $val -is [long] -or $val -is [uint32]){
      $iv=[int]$val;
      if($iv -gt 0 -and $iv -lt 8000){$script:o.days_remaining=$iv;$script:o.source=('registry:'+$path+'\'+$name)}
    }
  }
  if($name -match '(?i)expir|enddate|licenseend|subscri|renew|trialend'){
    try{
      $dt=[datetime]::Parse([string]$val);
      $days=[int]([timespan]($dt-(Get-Date))).TotalDays;
      if($days -ge -120 -and $days -lt 8000){$script:o.days_remaining=$days;$script:o.source=('registry:'+$path+'\'+$name)}
    }catch{}
  }
};
foreach($r in @('HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WRData')){
  if(-not (Test-Path $r)){continue};
  $o.installed=$true;
  try{
    $p=Get-ItemProperty $r -ErrorAction Stop;
    foreach($prop in $p.PSObject.Properties){
      $n=$prop.Name;
      if($n -match '^PS'){continue};
      if($n -match '(?i)day|expir|license|key|subscri|trial|renew|valid|end|hpl|gsm'){Add-Hit $r $n $prop.Value};
      Try-DaysFromNameValue $r $n $prop.Value
    }
    $nSub=0;
    Get-ChildItem $r -Recurse -Depth 2 -ErrorAction SilentlyContinue|ForEach-Object{
      if($nSub++ -gt 120){return};
      try{
        $g=Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue;
        foreach($prop in $g.PSObject.Properties){
          $n=$prop.Name;
          if($n -match '^PS'){continue};
          if($n -match '(?i)day|expir|license|key|subscri|trial|renew|valid|end'){
            Add-Hit $_.PSPath $n $prop.Value;
            Try-DaysFromNameValue $_.PSPath $n $prop.Value
          }
        }
      }catch{}
    }
  }catch{}
};
try{
  $av=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction Stop|Where-Object{$_.displayName -like '*Webroot*'}|Select-Object -First 1;
  if($av){
    $o.installed=$true;
    $st=[int]$av.productState;
    $prod=($st -shr 8) -band 0xFF;
    $sig=$st -band 0xFF;
    $o.wmi=[ordered]@{displayName=$av.displayName;instanceGuid=$av.instanceGuid;product_state_hex=('0x{0:x6}' -f $st);product_state=('0x{0:x2}' -f $prod);signature_state=('0x{0:x2}' -f $sig)};
    $o.active=(($prod -band 0x10) -ne 0);
    $o.definitions_up_to_date=($sig -eq 0);
    if(-not $o.active -and -not $o.note){$o.note='WSC: real-time protection off, expired, or snoozed.'}
  }
}catch{};
if(-not $o.installed){$o.note='Webroot not detected (no WRSA.exe / WRData / WSC listing).'}
elseif($null -eq $o.days_remaining -and (-not $o.note)){$o.note='Days remaining not parsed from registry; confirm in WRSA (gear > My Account / subscription).'};
$o|ConvertTo-Json -Compress -Depth 6"#;

const SAS_LICENSE_PS: &str = r#"$o=[ordered]@{product='superantispyware';installed=$false;active=$null;days_remaining=$null;source=$null;executable=$null;wmi=$null;registry_hits=@();note=$null};
$sasExe='C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe';
if(Test-Path $sasExe){$o.installed=$true;$o.executable=$sasExe};
function Add-Hit($path,$name,$val){
  if($val -is [byte[]]){return};
  $s=[string]$val;
  if($s.Length -gt 120){$s=$s.Substring(0,120)+'…'};
  $script:o.registry_hits+=[ordered]@{path=$path;name=$name;value=$s}
};
function Try-DaysFromNameValue($path,$name,$val){
  if($val -is [byte[]]){return};
  if($name -match '(?i)day|trial|valid'){
    if($val -is [int] -or $val -is [long] -or $val -is [uint32]){
      $iv=[int]$val;
      if($iv -gt 0 -and $iv -lt 8000){$script:o.days_remaining=$iv;$script:o.source=('registry:'+$path+'\'+$name)}
    }
  }
  if($name -match '(?i)expir|enddate|license|registr|renew|subscri'){
    try{
      $dt=[datetime]::Parse([string]$val);
      $days=[int]([timespan]($dt-(Get-Date))).TotalDays;
      if($days -ge -120 -and $days -lt 8000){$script:o.days_remaining=$days;$script:o.source=('registry:'+$path+'\'+$name)}
    }catch{}
  }
};
foreach($r in @('HKLM:\SOFTWARE\SUPERAntiSpyware','HKLM:\SOFTWARE\WOW6432Node\SUPERAntiSpyware')){
  if(-not (Test-Path $r)){continue};
  $o.installed=$true;
  try{
    $p=Get-ItemProperty $r -ErrorAction Stop;
    foreach($prop in $p.PSObject.Properties){
      $n=$prop.Name;
      if($n -match '^PS'){continue};
      if($n -match '(?i)day|expir|license|registr|trial|renew|valid|end|key|subscri'){Add-Hit $r $n $prop.Value};
      Try-DaysFromNameValue $r $n $prop.Value
    }
    $nSub=0;
    Get-ChildItem $r -Recurse -Depth 3 -ErrorAction SilentlyContinue|ForEach-Object{
      if($nSub++ -gt 120){return};
      try{
        $g=Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue;
        if(-not $g){return};
        foreach($prop in $g.PSObject.Properties){
          $n=$prop.Name;
          if($n -match '^PS'){continue};
          if($n -match '(?i)day|expir|license|registr|trial|renew|valid|end'){
            Add-Hit $_.PSPath $n $prop.Value;
            Try-DaysFromNameValue $_.PSPath $n $prop.Value
          }
        }
      }catch{}
    }
  }catch{}
};
try{
  $av=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction Stop|
    Where-Object{$_.displayName -like '*SUPERAntiSpyware*' -or $_.displayName -like '*SuperAntiSpyware*'}|
    Select-Object -First 1;
  if($av){
    $o.installed=$true;
    $st=[int]$av.productState;
    $prod=($st -shr 8) -band 0xFF;
    $sig=$st -band 0xFF;
    $o.wmi=[ordered]@{displayName=$av.displayName;instanceGuid=$av.instanceGuid;product_state_hex=('0x{0:x6}' -f $st);product_state=('0x{0:x2}' -f $prod);signature_state=('0x{0:x2}' -f $sig)};
    $o.active=(($prod -band 0x10) -ne 0);
    $o.definitions_up_to_date=($sig -eq 0);
    if(-not $o.active -and -not $o.note){$o.note='WSC: real-time protection off, expired, or snoozed.'}
  }
}catch{};
if(-not $o.installed){$o.note='SUPERAntiSpyware not detected.'}
elseif($null -eq $o.days_remaining -and (-not $o.note)){$o.note='Days remaining not parsed from registry; check Help > About / registration in SAS.'};
$o|ConvertTo-Json -Compress -Depth 6"#;

/// Base64 copies of SAS_ALLUSER.DB3 and every profile's SAS_CURRENTUSER.DB3, plus today's local date.
const SAS_DB_PS: &str = r#"$o=[ordered]@{today=(Get-Date).ToString('yyyy-MM-dd');alluser=$null;users=@()};
function Read-B64($p){$fs=[IO.File]::Open($p,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite);try{$b=New-Object byte[] $fs.Length;$n=0;while($n -lt $b.Length){$r=$fs.Read($b,$n,$b.Length-$n);if($r -le 0){break};$n+=$r};[Convert]::ToBase64String($b,0,$n)}finally{$fs.Close()}};
$a=Join-Path $env:ProgramData 'SUPERAntiSpyware.com\SUPERAntiSpyware\SAS_ALLUSER.DB3';
if(Test-Path $a){try{$o.alluser=[ordered]@{wal=(Test-Path ($a+'-wal'));b64=(Read-B64 $a)}}catch{$o.alluser=[ordered]@{error=$_.Exception.Message}}};
$users=New-Object System.Collections.ArrayList;
foreach($u in @(Get-ChildItem 'C:\Users' -Directory -ErrorAction SilentlyContinue)){
  $p=Join-Path $u.FullName 'AppData\Roaming\SUPERAntiSpyware.com\SUPERAntiSpyware\SAS_CURRENTUSER.DB3';
  if(Test-Path $p){try{[void]$users.Add([ordered]@{user=$u.Name;b64=(Read-B64 $p)})}catch{[void]$users.Add([ordered]@{user=$u.Name;error=$_.Exception.Message})}}
};
$o.users=$users.ToArray();
$o|ConvertTo-Json -Compress -Depth 4"#;

const WSC_PRODUCTS_PS: &str = r#"$rows=@();
foreach($cls in @('AntiVirusProduct','AntiSpywareProduct','FirewallProduct')){
  try{
    Get-CimInstance -Namespace root/SecurityCenter2 -ClassName $cls -EA Stop | ForEach-Object {
      $st=[int]$_.productState;
      $prod=($st -shr 8) -band 0xFF;
      $sig=$st -band 0xFF;
      $exe=[string]$_.pathToSignedProductExe;
      $exeReal=if($exe){$exe -replace '^windowsdefender://',''}else{''};
      $missing=$false;
      if($exeReal -and ($exeReal -notmatch '://') -and -not (Test-Path $exeReal)){$missing=$true};
      $rows+=[ordered]@{
        class=$cls;
        displayName=$_.displayName;
        instanceGuid=$_.instanceGuid;
        product_state_hex=('0x{0:x6}' -f $st);
        real_time_on=(($prod -band 0x10) -ne 0);
        definitions_up_to_date=($sig -eq 0);
        exe=$exe;
        exe_missing=$missing
      }
    }
  }catch{}
};
[PSCustomObject]@{ products=@($rows); count=@($rows).Count } | ConvertTo-Json -Compress -Depth 5"#;

const WEBROOT_STATUS_DEEP_PS: &str = r#"$o=[ordered]@{services=@();driver=$null;process=$null;install_dirs=@();reg_keys=@();sc_registrations=@();sc_ghost_count=0;scheduled_tasks=@()};
$o.services=Get-Service -EA SilentlyContinue|Where-Object{$_.Name -match '(?i)wr|webroot'}|Select-Object Name,DisplayName,Status,StartType;
$drv=Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn' -EA SilentlyContinue;
if($drv){$o.driver=[ordered]@{name='WRkrn';image=$drv.ImagePath;start=$drv.Start}};
$o.process=Get-Process WRSA -EA SilentlyContinue|Select-Object Id,@{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}};
foreach($d in @('C:\Program Files\Webroot','C:\Program Files (x86)\Webroot','C:\ProgramData\WRData','C:\ProgramData\WRCore')){if(Test-Path $d){$o.install_dirs+=$d}};
foreach($k in @('HKLM:\SOFTWARE\WRData','HKLM:\SOFTWARE\WRCore','HKLM:\SOFTWARE\WRMIDData','HKLM:\SOFTWARE\Webroot','HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WOW6432Node\WRCore','HKLM:\SOFTWARE\WOW6432Node\Webroot')){if(Test-Path $k){$o.reg_keys+=$k}};
try{$sc=Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -EA Stop|Where-Object{$_.displayName -like '*Webroot*'}|Select-Object displayName,@{N='state';E={'0x{0:x8}' -f [int]$_.productState}},instanceGuid,pathToSignedProductExe;$o.sc_registrations=@($sc);$o.sc_ghost_count=@($sc).Count}catch{};
$o.scheduled_tasks=Get-ScheduledTask -EA SilentlyContinue|Where-Object{$_.TaskName -match '(?i)webroot|wrsa'}|Select-Object TaskName,State;
$o|ConvertTo-Json -Compress -Depth 5"#;

const WEBROOT_CLEANUP_PS: &str = r#"$uns=@('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*');
function UninstallEntries { @(Get-ItemProperty $uns -EA SilentlyContinue | Where-Object {$_.DisplayName -like '*Webroot*'}) };
function StillInstalled { (Test-Path 'C:\Program Files\Webroot') -or (Test-Path 'C:\Program Files (x86)\Webroot') -or (@(Get-Service WRSVC,WRCoreService,WRSkyClient,WRBoot -EA SilentlyContinue).Count -gt 0) -or (@(Get-Process WRSA -EA SilentlyContinue).Count -gt 0) };
function ScCount { try{ @(Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -EA Stop | Where-Object {$_.displayName -like '*Webroot*'}).Count }catch{ -1 } };
$before=[ordered]@{installed=(StillInstalled);uninstall_entries=(UninstallEntries).Count;sc_registrations=(ScCount)};
$r=@();
function Step($n,$b,$v){
  $script:detail=$null; $err=$null;
  try{ & $b } catch { $err=$_.Exception.Message };
  $still=$true; try{ $still=[bool](& $v) } catch { $still=$true };
  $rec=[ordered]@{step=$n;ok=(-not $still);removed=(-not $still)};
  if($still){$rec.blocked=$true};
  if($script:detail){$rec.detail=(('{0}' -f $script:detail).Trim())};
  if($err){$rec.err=$err};
  $script:r+=$rec
};
Step 'kill_wrsa' { Get-Process WRSA,WRSACt64,WRSACt32 -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue } { @(Get-Process WRSA,WRSACt64,WRSACt32 -EA SilentlyContinue).Count -gt 0 };
foreach($svc in @('WRSVC','WRCoreService','WRSkyClient','WRBoot')){
  if(Get-Service $svc -EA SilentlyContinue){
    $sv=$svc;
    Step ('stop_'+$sv) { Stop-Service $sv -Force -EA SilentlyContinue } { (Get-Service $sv -EA SilentlyContinue).Status -eq 'Running' };
    Step ('delete_'+$sv) { $script:detail = (& sc.exe delete $sv 2>&1 | Out-String) } { @(Get-Service $sv -EA SilentlyContinue).Count -gt 0 }
  }
};
Step 'delete_WRkrn_driver' { if(Test-Path 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn'){$script:detail = (& sc.exe delete WRkrn 2>&1 | Out-String)} } { Test-Path 'HKLM:\SYSTEM\CurrentControlSet\Services\WRkrn' };
foreach($d in @('C:\Program Files\Webroot','C:\Program Files (x86)\Webroot','C:\ProgramData\WRData','C:\ProgramData\WRCore','C:\ProgramData\WRMIDData')){
  if(Test-Path $d){ $dd=$d; Step ('rmdir_'+$dd) { Remove-Item $dd -Recurse -Force -EA SilentlyContinue } { Test-Path $dd } }
};
foreach($k in @('HKLM:\SOFTWARE\WRData','HKLM:\SOFTWARE\WRCore','HKLM:\SOFTWARE\WRMIDData','HKLM:\SOFTWARE\Webroot','HKLM:\SOFTWARE\WOW6432Node\WRData','HKLM:\SOFTWARE\WOW6432Node\WRCore','HKLM:\SOFTWARE\WOW6432Node\WRMIDData','HKLM:\SOFTWARE\WOW6432Node\Webroot')){
  if(Test-Path $k){ $kk=$k; Step ('rmreg_'+$kk) { Remove-Item $kk -Recurse -Force -EA SilentlyContinue } { Test-Path $kk } }
};
Step 'remove_uninstall_entries' { if(-not (StillInstalled)){ (UninstallEntries) | ForEach-Object { Remove-Item $_.PSPath -Recurse -Force -EA SilentlyContinue } } else { $script:detail='skipped: Webroot still installed — leaving the Uninstall entry to avoid stranding the product' } } { if(StillInstalled){ $false } else { (UninstallEntries).Count -gt 0 } };
foreach($run in @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run')){
  $rk=$run;
  Step ('clean_run_'+$rk) {
    $p=Get-ItemProperty $rk -EA SilentlyContinue;
    if($p){ $p.PSObject.Properties | Where-Object {$_.Name -match '(?i)webroot|wrsa'} | ForEach-Object { Remove-ItemProperty -Path $rk -Name $_.Name -EA SilentlyContinue } }
  } {
    $p=Get-ItemProperty $rk -EA SilentlyContinue;
    if(-not $p){ $false } else { @($p.PSObject.Properties | Where-Object {$_.Name -match '(?i)webroot|wrsa'}).Count -gt 0 }
  }
};
Step 'remove_scheduled_tasks' { Get-ScheduledTask -EA SilentlyContinue | Where-Object {$_.TaskName -match '(?i)webroot|wrsa'} | Unregister-ScheduledTask -Confirm:$false -EA SilentlyContinue } { @(Get-ScheduledTask -EA SilentlyContinue | Where-Object {$_.TaskName -match '(?i)webroot|wrsa'}).Count -gt 0 };
$after=[ordered]@{installed=(StillInstalled);uninstall_entries=(UninstallEntries).Count;sc_registrations=(ScCount)};
$blocked=@($r | Where-Object {-not $_.ok}).Count;
$warn=@();
if($blocked -gt 0){$warn+=('Webroot self-protection blocked {0} of {1} steps; ok:false means the target is still present.' -f $blocked,@($r).Count)};
if($after.installed){$warn+='Webroot is still installed. Shop policy is that Webroot stays - if this run was not the first half of a planned reinstall, stop here.'};
if($after.sc_registrations -gt 0){$warn+=('{0} Webroot Security Center registration(s) remain. This tool never touches root/SecurityCenter2 and cannot clear ghost WSC entries.' -f $after.sc_registrations)};
[ordered]@{steps=@($r);before=$before;after=$after;blocked_steps=$blocked;warnings=$warn}|ConvertTo-Json -Compress -Depth 6"#;

fn webroot_license() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] webroot_license");
    Ok(envelope(
        "webroot_license",
        host::run_command(WEBROOT_LICENSE_PS),
    ))
}

fn sas_license() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] sas_license");
    let mut out = envelope("sas_license", host::run_command(SAS_LICENSE_PS));
    let db = serde_json::from_str::<serde_json::Value>(host::run_command(SAS_DB_PS).trim())
        .unwrap_or_default();
    if let Some(output) = out
        .get_mut("output")
        .and_then(serde_json::Value::as_object_mut)
    {
        merge_sas_db(output, &db);
    }
    Ok(out)
}

fn wsc_products() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] wsc_products");
    Ok(envelope("wsc_products", host::run_command(WSC_PRODUCTS_PS)))
}

fn webroot_status_deep() -> Result<serde_json::Value, SdkError> {
    host::log("[cps] webroot_status_deep");
    Ok(envelope(
        "webroot_status_deep",
        host::run_command(WEBROOT_STATUS_DEEP_PS),
    ))
}

fn webroot_cleanup(a: CleanupArgs) -> Result<serde_json::Value, SdkError> {
    if a.confirm_full_uninstall != Some(true) {
        return Err(SdkError::invalid_args(
            "webroot_cleanup is a FULL uninstall of Webroot (software the shop sells), not remnant pruning and not a ghost-WSC fix. Pass confirm_full_uninstall:true only as the first half of a deliberate repair-and-reinstall with a re-key in hand.",
        ));
    }
    host::log("[cps] webroot_cleanup (confirmed)");
    Ok(envelope(
        "webroot_cleanup",
        host::run_command(WEBROOT_CLEANUP_PS),
    ))
}

mtech_plugin! {
    id: "com.mastertech.cps",
    name: "CPS Security Software",
    version: "0.2.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// CPS / Webroot: installed state, Windows Security Center real-time protection, and days-remaining from registry heuristics.
        webroot_license() => webroot_license,
        /// CPS / SUPERAntiSpyware: registration, subscription expiry and days remaining from SAS's own SETTINGS database, plus per-user real-time protection. Never returns the registration code.
        sas_license() => sas_license,
        /// All Windows Security Center products (AV/AntiSpyware/Firewall) with correctly decoded real-time and signature state, flagging any whose product exe is missing.
        wsc_products() => wsc_products,
        /// Deep Webroot remnant survey: services, WRkrn driver, install dirs, registry keys, ALL Security Center registrations (ghost count), WRSA process, scheduled tasks. Read-only.
        webroot_status_deep() => webroot_status_deep,
        /// DESTRUCTIVE full uninstall of Webroot SecureAnywhere. Skips deleting the Uninstall entry while the product is still installed (avoids stranding). Does NOT clear ghost WSC registrations. Requires confirm_full_uninstall:true.
        webroot_cleanup(CleanupArgs) => webroot_cleanup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTERED: &[u8] = include_bytes!("../tests/fixtures/sas_alluser_registered.db3");
    const UNREGISTERED: &[u8] = include_bytes!("../tests/fixtures/sas_alluser_unregistered.db3");
    const CURRENT_USER: &[u8] = include_bytes!("../tests/fixtures/sas_currentuser.db3");

    fn b64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b))
                << (8 * (3 - chunk.len()));
            for i in 0..4 {
                s.push(if i <= chunk.len() {
                    ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char
                } else {
                    '='
                });
            }
        }
        s
    }

    fn merged(alluser: &[u8]) -> serde_json::Map<String, serde_json::Value> {
        let mut output = serde_json::Map::new();
        let db = serde_json::json!({
            "today": "2026-09-30",
            "alluser": { "wal": false, "b64": b64(alluser) },
            "users": [{ "user": "Owner", "b64": b64(CURRENT_USER) }],
        });
        merge_sas_db(&mut output, &db);
        output
    }

    #[test]
    fn reads_rows_across_interior_and_overflow_pages() {
        let rows = sqlite::table_rows(REGISTERED, "settings").unwrap();
        assert_eq!(rows.len(), 131);
        let big = rows
            .iter()
            .find(|r| r[1] == sqlite::Value::Text(b"BigBlob".to_vec()))
            .unwrap();
        let expected: Vec<u8> = (0..=255u8).cycle().take(2048).collect();
        assert_eq!(big[3], sqlite::Value::Blob(expected));
    }

    #[test]
    fn rejects_bytes_that_are_not_sqlite() {
        assert!(sqlite::table_rows(b"not a database at all", "SETTINGS").is_err());
        assert!(sqlite::table_rows(REGISTERED, "MISSING").is_err());
    }

    #[test]
    fn registered_database_reports_its_expiry() {
        let reg = sas::registration(&sas::settings(REGISTERED).unwrap());
        assert!(reg.registered);
        assert_eq!(reg.expires, sas::Date::new(2027, 9, 29));
        assert_eq!(reg.expire_to_free, Some(false));
    }

    #[test]
    fn unregistered_database_has_no_code_or_expiry() {
        let reg = sas::registration(&sas::settings(UNREGISTERED).unwrap());
        assert!(!reg.registered);
        assert_eq!(reg.expires, None);
    }

    #[test]
    fn current_user_database_reports_real_time_and_pro_flags() {
        assert_eq!(
            sas::user_state(&sas::settings(CURRENT_USER).unwrap()),
            (Some(true), Some(true))
        );
    }

    #[test]
    fn days_until_counts_calendar_days() {
        let today = sas::Date::parse("2026-09-30").unwrap();
        assert_eq!(today.days_until(sas::Date::new(2027, 9, 29).unwrap()), 364);
        assert_eq!(today.days_until(sas::Date::new(2026, 9, 29).unwrap()), -1);
        let leap = sas::Date::new(2024, 2, 28).unwrap();
        assert_eq!(leap.days_until(sas::Date::new(2024, 3, 1).unwrap()), 2);
        assert_eq!(sas::Date::parse("0000-00-00"), None);
    }

    #[test]
    fn base64_round_trips_a_database() {
        assert_eq!(b64_decode(&b64(REGISTERED)).unwrap(), REGISTERED);
        assert_eq!(b64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert!(b64_decode("SGV*bG8=").is_err());
    }

    #[test]
    fn merge_reports_registration_without_the_code() {
        let output = merged(REGISTERED);
        assert_eq!(output["registered"], true);
        assert_eq!(output["subscription_expiration"], "2027-09-29");
        assert_eq!(output["days_remaining"], 364);
        assert_eq!(output["expired"], false);
        assert_eq!(output["real_time_on"], true);
        assert_eq!(output["active"], true);
        assert_eq!(output["note"], serde_json::Value::Null);
        assert!(
            !serde_json::Value::Object(output)
                .to_string()
                .contains("TESTREGCODE0001")
        );
    }

    #[test]
    fn merge_flags_the_free_edition() {
        let output = merged(UNREGISTERED);
        assert_eq!(output["registered"], false);
        assert_eq!(output["active"], false);
        assert_eq!(output["days_remaining"], serde_json::Value::Null);
        assert!(output["note"].as_str().unwrap().starts_with("Unregistered"));
    }

    #[test]
    fn merge_keeps_registry_output_when_sas_is_absent() {
        let mut output = serde_json::Map::new();
        output.insert(
            "note".into(),
            serde_json::json!("SUPERAntiSpyware not detected."),
        );
        merge_sas_db(
            &mut output,
            &serde_json::json!({ "today": "2026-09-30", "alluser": null, "users": [] }),
        );
        assert_eq!(output["note"], "SUPERAntiSpyware not detected.");
        assert!(!output.contains_key("registered"));
    }

    #[test]
    fn merge_reports_an_unreadable_database() {
        let mut output = serde_json::Map::new();
        merge_sas_db(
            &mut output,
            &serde_json::json!({ "alluser": { "error": "locked" } }),
        );
        assert_eq!(output["db_error"], "locked");
    }
}

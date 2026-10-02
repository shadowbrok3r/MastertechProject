//! Exact multiplication mod a 50-bit prime: an AVX2/FMA3 `f64` kernel and an integer reference.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest prime below 2^50.
pub(crate) const P: u64 = (1 << 50) - 27;
const LOW50: u64 = (1 << 50) - 1;
/// 2^50 mod P.
const FOLD: u64 = 27;
/// Vectors per block handed to [`run_pass`].
pub(crate) const BLOCK: usize = 64;
/// Lane multiplies counted as this many floating-point operations.
pub(crate) const FLOPS_PER_MUL: f64 = 8.0;

/// Four residues, aligned for 256-bit loads and stores.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C, align(32))]
pub(crate) struct Lane4(pub [f64; 4]);

/// `a·b mod P` for `a, b < P`, by pseudo-Mersenne folding.
#[inline]
pub(crate) fn mul_mod(a: u64, b: u64) -> u64 {
    let x = u128::from(a) * u128::from(b);
    let y = (x >> 50) as u64 * FOLD + (x as u64 & LOW50);
    let r = (y >> 50) * FOLD + (y & LOW50);
    if r >= P { r - P } else { r }
}

/// `base^exp mod P` for `base < P`.
pub(crate) fn pow_mod(mut base: u64, mut exp: u64) -> u64 {
    let mut acc = 1;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = mul_mod(acc, base);
        }
        base = mul_mod(base, base);
        exp >>= 1;
    }
    acc
}

/// Next xorshift residue in `[2, P)`; `state` must be nonzero.
pub(crate) fn residue(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    2 + *state % (P - 2)
}

/// `true` when the CPU and OS support AVX2 and FMA3.
pub(crate) fn avx2_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Instruction path a pass runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Path {
    Avx2,
    Scalar,
}

impl Path {
    pub(crate) fn detect() -> Self {
        if avx2_available() {
            Self::Avx2
        } else {
            Self::Scalar
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Avx2 => "AVX2+FMA3",
            Self::Scalar => "scalar",
        }
    }
}

/// Lane `i` enters holding `(i+1)·g_in` and leaves holding `(i+1)·g_in·c^rounds`, mod P.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pass {
    pub g_in: u64,
    pub c: u64,
    pub rounds: u32,
    pub g_out: u64,
}

impl Pass {
    pub(crate) fn new(g_in: u64, c: u64, rounds: u32) -> Self {
        Self {
            g_in,
            c,
            rounds,
            g_out: mul_mod(g_in, pow_mod(c, u64::from(rounds))),
        }
    }

    /// Pass that enters with this pass's output.
    pub(crate) fn then(&self, c: u64) -> Self {
        Self::new(self.g_out, c, self.rounds)
    }

    /// Lane multiplies, checks included, for `lanes` lanes.
    pub(crate) fn mul_count(&self, lanes: u64) -> u64 {
        lanes * (u64::from(self.rounds) + 2)
    }
}

/// Which check caught a mismatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    /// The value read back is not the value written.
    Stored,
    /// The value read back was right and the arithmetic was not.
    Computed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mismatch {
    pub fault: Fault,
    /// Lane index within the worker's buffer.
    pub index: u64,
    pub expected: u64,
    /// Bits of the wrong `f64`.
    pub found: u64,
}

/// Writes `(i+1)·g` into every lane; `base` is the index of the first lane.
pub(crate) fn fill(path: Path, lanes: &mut [Lane4], base: u64, g: u64) {
    match path {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 and FMA3 were just detected.
        Path::Avx2 if avx2_available() => unsafe { simd::fill(lanes, base, g) },
        _ => scalar::fill(lanes, base, g),
    }
}

/// Runs `pass` over `block`, rewriting and reporting every lane that fails a check.
pub(crate) fn run_pass(
    path: Path,
    block: &mut [Lane4],
    base: u64,
    pass: &Pass,
    faults: &mut Vec<Mismatch>,
) {
    match path {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: AVX2 and FMA3 were just detected.
        Path::Avx2 if avx2_available() => unsafe { simd::run_pass(block, base, pass, faults) },
        _ => scalar::run_pass(block, base, pass, faults),
    }
}

/// Rewrites lanes not holding `(i+1)·g`; a right lane under a wrong check is a `Computed` fault.
fn correct(
    lane: &mut Lane4,
    computed: [f64; 4],
    first: u64,
    g: u64,
    fault: Fault,
    faults: &mut Vec<Mismatch>,
) {
    for (k, (v, check)) in lane.0.iter_mut().zip(computed).enumerate() {
        let index = first + k as u64;
        let expected = mul_mod(index + 1, g);
        if *v != expected as f64 {
            faults.push(Mismatch {
                fault,
                index,
                expected,
                found: v.to_bits(),
            });
            *v = expected as f64;
        } else if check != expected as f64 {
            faults.push(Mismatch {
                fault: Fault::Computed,
                index,
                expected,
                found: check.to_bits(),
            });
        }
    }
}

mod scalar {
    use super::{BLOCK, Fault, Lane4, Mismatch, Pass, mul_mod};

    pub(super) fn fill(lanes: &mut [Lane4], base: u64, g: u64) {
        for (j, lane) in lanes.iter_mut().enumerate() {
            for (k, v) in lane.0.iter_mut().enumerate() {
                *v = mul_mod(base + (4 * j + k) as u64 + 1, g) as f64;
            }
        }
    }

    pub(super) fn run_pass(
        lanes: &mut [Lane4],
        base: u64,
        pass: &Pass,
        faults: &mut Vec<Mismatch>,
    ) {
        let mut work = [0u64; BLOCK * 4];
        for (b, block) in lanes.chunks_mut(BLOCK).enumerate() {
            let first = base + (b * BLOCK * 4) as u64;
            let work = &mut work[..block.len() * 4];
            for (i, (w, v)) in work
                .iter_mut()
                .zip(block.iter().flat_map(|l| l.0))
                .enumerate()
            {
                let index = first + i as u64;
                *w = mul_mod(index + 1, pass.g_in);
                if v != *w as f64 {
                    faults.push(Mismatch {
                        fault: Fault::Stored,
                        index,
                        expected: *w,
                        found: v.to_bits(),
                    });
                }
            }
            for _ in 0..pass.rounds {
                for w in work.iter_mut() {
                    *w = mul_mod(*w, pass.c);
                }
            }
            for (i, (w, v)) in work
                .iter()
                .zip(block.iter_mut().flat_map(|l| l.0.iter_mut()))
                .enumerate()
            {
                let index = first + i as u64;
                let expected = mul_mod(index + 1, pass.g_out);
                if *w != expected {
                    faults.push(Mismatch {
                        fault: Fault::Computed,
                        index,
                        expected,
                        found: (*w as f64).to_bits(),
                    });
                }
                *v = expected as f64;
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod simd {
    use std::arch::x86_64::*;

    use super::{Fault, Lane4, Mismatch, P, Pass, correct};

    /// `a·b mod P` per lane for residues below P.
    #[target_feature(enable = "avx2,fma")]
    fn mul(a: __m256d, b: __m256d, p: __m256d, pinv: __m256d) -> __m256d {
        let h = _mm256_mul_pd(a, b);
        let l = _mm256_fmsub_pd(a, b, h);
        let q = _mm256_round_pd::<{ _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC }>(
            _mm256_mul_pd(h, pinv),
        );
        let r = _mm256_add_pd(_mm256_fnmadd_pd(q, p, h), l);
        let negative = _mm256_cmp_pd::<_CMP_LT_OQ>(r, _mm256_setzero_pd());
        _mm256_add_pd(r, _mm256_and_pd(negative, p))
    }

    /// Ordinals `first+1 ..= first+4`.
    #[target_feature(enable = "avx2,fma")]
    fn ordinals(first: u64) -> __m256d {
        _mm256_add_pd(
            _mm256_set1_pd(first as f64),
            _mm256_set_pd(4.0, 3.0, 2.0, 1.0),
        )
    }

    #[target_feature(enable = "avx2,fma")]
    pub(super) fn fill(lanes: &mut [Lane4], base: u64, g: u64) {
        let (p, pinv) = (_mm256_set1_pd(P as f64), _mm256_set1_pd(1.0 / P as f64));
        let g = _mm256_set1_pd(g as f64);
        for (j, lane) in lanes.iter_mut().enumerate() {
            let v = mul(ordinals(base + 4 * j as u64), g, p, pinv);
            // SAFETY: `Lane4` is four `f64` at 32-byte alignment.
            unsafe { _mm256_store_pd(lane.0.as_mut_ptr(), v) };
        }
    }

    #[target_feature(enable = "avx2,fma")]
    pub(super) fn run_pass(
        block: &mut [Lane4],
        base: u64,
        pass: &Pass,
        faults: &mut Vec<Mismatch>,
    ) {
        let (p, pinv) = (_mm256_set1_pd(P as f64), _mm256_set1_pd(1.0 / P as f64));
        let c = _mm256_set1_pd(pass.c as f64);
        check(block, base, pass.g_in, Fault::Stored, p, pinv, faults);
        for _ in 0..pass.rounds {
            for lane in block.iter_mut() {
                let v = lane.0.as_mut_ptr();
                // SAFETY: `Lane4` is four `f64` at 32-byte alignment.
                unsafe { _mm256_store_pd(v, mul(_mm256_load_pd(v), c, p, pinv)) };
            }
        }
        check(block, base, pass.g_out, Fault::Computed, p, pinv, faults);
    }

    /// Compares every lane with `(i+1)·g`, correcting the ones that differ.
    #[target_feature(enable = "avx2,fma")]
    fn check(
        block: &mut [Lane4],
        base: u64,
        g: u64,
        fault: Fault,
        p: __m256d,
        pinv: __m256d,
        faults: &mut Vec<Mismatch>,
    ) {
        let gv = _mm256_set1_pd(g as f64);
        for (j, lane) in block.iter_mut().enumerate() {
            let first = base + 4 * j as u64;
            // SAFETY: `Lane4` is four `f64` at 32-byte alignment.
            let found = unsafe { _mm256_load_pd(lane.0.as_ptr()) };
            let want = mul(ordinals(first), gv, p, pinv);
            if _mm256_movemask_pd(_mm256_cmp_pd::<_CMP_NEQ_UQ>(found, want)) != 0 {
                let mut computed = [0.0; 4];
                // SAFETY: `computed` is four `f64`.
                unsafe { _mm256_storeu_pd(computed.as_mut_ptr(), want) };
                correct(lane, computed, first, g, fault, faults);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn mul_lanes(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
        #[target_feature(enable = "avx2,fma")]
        fn inner(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
            let (p, pinv) = (_mm256_set1_pd(P as f64), _mm256_set1_pd(1.0 / P as f64));
            let mut out = [0.0; 4];
            // SAFETY: each array is four `f64`.
            unsafe {
                let r = mul(
                    _mm256_loadu_pd(a.as_ptr()),
                    _mm256_loadu_pd(b.as_ptr()),
                    p,
                    pinv,
                );
                _mm256_storeu_pd(out.as_mut_ptr(), r);
            }
            out
        }
        assert!(super::avx2_available());
        // SAFETY: AVX2 and FMA3 were just asserted.
        unsafe { inner(a, b) }
    }
}

/// Error tally shared by one stressor's workers.
pub(crate) struct ErrorLog {
    tag: &'static str,
    /// Word for a `Stored` fault in messages.
    stored: &'static str,
    count: AtomicU64,
    detail: Mutex<Detail>,
}

#[derive(Default)]
struct Detail {
    last: Option<String>,
    by_cpu: BTreeMap<usize, u64>,
}

/// Logical CPUs named in the per-CPU tally.
const TALLY_CPUS: usize = 4;

impl ErrorLog {
    pub(crate) fn new(tag: &'static str, stored: &'static str) -> Self {
        Self {
            tag,
            stored,
            count: AtomicU64::new(0),
            detail: Mutex::new(Detail::default()),
        }
    }

    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub(crate) fn last(&self) -> Option<String> {
        self.detail.lock().ok().and_then(|d| d.last.clone())
    }

    /// Counts `faults` found by `worker` on logical CPU `cpu`.
    pub(crate) fn record(&self, worker: usize, cpu: Option<usize>, faults: &[Mismatch]) {
        let Some(newest) = faults.last() else { return };
        let n = faults.len() as u64;
        self.count.fetch_add(n, Ordering::Relaxed);
        let Ok(mut detail) = self.detail.lock() else {
            return;
        };
        if let Some(cpu) = cpu {
            *detail.by_cpu.entry(cpu).or_default() += n;
        }
        let msg = self.describe(worker, cpu, newest, n - 1, &detail.by_cpu);
        log::error!("[stress-kit/{}] {msg}", self.tag);
        detail.last = Some(msg);
    }

    fn describe(
        &self,
        worker: usize,
        cpu: Option<usize>,
        m: &Mismatch,
        others: u64,
        by_cpu: &BTreeMap<usize, u64>,
    ) -> String {
        let (kind, location) = match m.fault {
            Fault::Stored => (self.stored, format!("offset 0x{:X}", m.index * 8)),
            Fault::Computed => ("compute", format!("lane {}", m.index)),
        };
        let expected = (m.expected as f64).to_bits();
        let bits = (expected ^ m.found).count_ones();
        let on_cpu = cpu
            .map(|c| format!(" on logical CPU {c}"))
            .unwrap_or_default();
        let more = if others > 0 {
            format!(" (+{others} more in this block)")
        } else {
            String::new()
        };
        let mut msg = format!(
            "{}[{worker}] {kind} error{on_cpu}: {location} expected 0x{expected:016X} got 0x{:016X} \
             ({bits} bit(s) differ){more}",
            self.tag, m.found
        );
        if !by_cpu.is_empty() {
            let mut ranked: Vec<(usize, u64)> = by_cpu.iter().map(|(c, n)| (*c, *n)).collect();
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let tally: Vec<String> = ranked
                .iter()
                .take(TALLY_CPUS)
                .map(|(c, n)| format!("CPU {c}: {n}"))
                .collect();
            msg.push_str(&format!("; errors by logical CPU: {}", tally.join(", ")));
        }
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(a: u64, b: u64) -> u64 {
        ((u128::from(a) * u128::from(b)) % u128::from(P)) as u64
    }

    fn edge_and_random_pairs() -> Vec<(u64, u64)> {
        let mut pairs = vec![
            (0, 0),
            (1, 1),
            (P - 1, P - 1),
            (P - 1, 1),
            (2, P.div_ceil(2)),
            (P - 2, P - 3),
        ];
        let mut state = 0x2545_F491_4F6C_DD1D;
        for _ in 0..20_000 {
            pairs.push((residue(&mut state), residue(&mut state)));
        }
        pairs
    }

    #[test]
    fn modulus_is_prime() {
        let n = P;
        assert!(
            (2..)
                .take_while(|d| d * d <= 1 << 20)
                .all(|d| !n.is_multiple_of(d))
        );
        let d = (n - 1) >> (n - 1).trailing_zeros();
        let s = (n - 1).trailing_zeros();
        for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
            let mut x = pow_mod(a, d);
            if x == 1 || x == n - 1 {
                continue;
            }
            let witness = (1..s).all(|_| {
                x = mul_mod(x, x);
                x != n - 1
            });
            assert!(!witness, "{a} witnesses that P is composite");
        }
    }

    #[test]
    fn mul_mod_matches_u128_remainder() {
        for (a, b) in edge_and_random_pairs() {
            assert_eq!(mul_mod(a, b), reference(a, b), "{a} * {b}");
        }
    }

    #[test]
    fn pass_output_multiplier_is_c_to_the_rounds() {
        let pass = Pass::new(12_345, 678, 3);
        assert_eq!(
            pass.g_out,
            reference(reference(reference(12_345, 678), 678), 678)
        );
        assert_eq!(pass.then(9).g_in, pass.g_out);
    }

    #[test]
    fn avx2_lanes_match_the_integer_reference() {
        if !avx2_available() {
            return;
        }
        for chunk in edge_and_random_pairs().as_chunks::<4>().0 {
            let a = chunk.map(|(a, _)| a as f64);
            let b = chunk.map(|(_, b)| b as f64);
            let got = simd::mul_lanes(a, b);
            for (k, (x, y)) in chunk.iter().enumerate() {
                assert_eq!(got[k], reference(*x, *y) as f64, "{x} * {y}");
            }
        }
    }

    fn filled(path: Path, vectors: usize, g: u64) -> Vec<Lane4> {
        let mut buf = vec![Lane4::default(); vectors];
        fill(path, &mut buf, 0, g);
        buf
    }

    #[test]
    fn both_paths_fill_and_pass_identically() {
        let mut pass = Pass::new(0x1234_5678_9ABC, 0xFEDC_BA98, 5);
        let mut scalar = filled(Path::Scalar, BLOCK * 3 + 7, pass.g_in);
        let mut vector = filled(Path::detect(), BLOCK * 3 + 7, pass.g_in);
        assert_eq!(scalar, vector);
        let mut faults = Vec::new();
        for c in [3, P - 2, 77] {
            for (b, block) in scalar.chunks_mut(BLOCK).enumerate() {
                run_pass(
                    Path::Scalar,
                    block,
                    (b * BLOCK * 4) as u64,
                    &pass,
                    &mut faults,
                );
            }
            for (b, block) in vector.chunks_mut(BLOCK).enumerate() {
                run_pass(
                    Path::detect(),
                    block,
                    (b * BLOCK * 4) as u64,
                    &pass,
                    &mut faults,
                );
            }
            assert!(faults.is_empty(), "clean buffers reported {faults:?}");
            assert_eq!(scalar, vector);
            pass = pass.then(c);
        }
        assert_eq!(scalar, filled(Path::Scalar, BLOCK * 3 + 7, pass.g_in));
    }

    #[test]
    fn a_corrupted_lane_is_a_stored_fault_and_is_repaired() {
        for path in [Path::Scalar, Path::detect()] {
            let pass = Pass::new(987_654_321, 31_337, 4);
            let mut buf = filled(path, BLOCK, pass.g_in);
            let good = buf[9].0[2];
            buf[9].0[2] = f64::from_bits(good.to_bits() ^ (1 << 7));
            let mut faults = Vec::new();
            run_pass(path, &mut buf, 0, &pass, &mut faults);
            assert_eq!(faults.len(), 1, "{path:?}: {faults:?}");
            assert_eq!(faults[0].fault, Fault::Stored);
            assert_eq!(faults[0].index, 9 * 4 + 2);
            assert_eq!(faults[0].found, good.to_bits() ^ (1 << 7));
            assert_eq!(
                buf,
                filled(Path::Scalar, BLOCK, pass.g_out),
                "{path:?} left the lane wrong"
            );
        }
    }

    #[test]
    fn a_wrong_result_is_a_compute_fault() {
        for path in [Path::Scalar, Path::detect()] {
            let honest = Pass::new(55_555, 444, 2);
            let lying = Pass {
                g_out: mul_mod(honest.g_out, 2),
                ..honest
            };
            let mut buf = filled(path, 2, honest.g_in);
            let mut faults = Vec::new();
            run_pass(path, &mut buf, 0, &lying, &mut faults);
            assert_eq!(faults.len(), 8, "{path:?}: {faults:?}");
            assert!(faults.iter().all(|m| m.fault == Fault::Computed));
            assert_eq!(buf, filled(Path::Scalar, 2, lying.g_out));
        }
    }

    #[test]
    fn a_wrong_check_value_on_a_good_lane_is_a_compute_fault() {
        let g = 4_242;
        let mut lane = filled(Path::Scalar, 1, g)[0];
        let mut computed = lane.0;
        computed[1] += 1.0;
        let mut faults = Vec::new();
        correct(&mut lane, computed, 0, g, Fault::Stored, &mut faults);
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].fault, Fault::Computed);
        assert_eq!(faults[0].found, computed[1].to_bits());
        assert_eq!(
            lane,
            filled(Path::Scalar, 1, g)[0],
            "a good lane must not be rewritten"
        );
    }

    #[test]
    fn error_log_names_the_fault_and_tallies_cpus() {
        let log = ErrorLog::new("cpu_mem", "memory");
        let m = Mismatch {
            fault: Fault::Stored,
            index: 0x10,
            expected: 5,
            found: 5f64.to_bits() ^ 1,
        };
        log.record(2, Some(7), &[m, m]);
        log.record(
            3,
            Some(12),
            &[Mismatch {
                fault: Fault::Computed,
                ..m
            }],
        );
        assert_eq!(log.count(), 3);
        let msg = log.last().expect("message recorded");
        assert!(
            msg.starts_with("cpu_mem[3] compute error on logical CPU 12: lane 16"),
            "{msg}"
        );
        assert!(msg.contains("(1 bit(s) differ)"), "{msg}");
        assert!(
            msg.ends_with("errors by logical CPU: CPU 7: 2, CPU 12: 1"),
            "{msg}"
        );
        let first = ErrorLog::new("cpu_mem", "memory");
        first.record(0, None, &[m, m, m]);
        let msg = first.last().expect("message recorded");
        assert!(msg.contains("memory error: offset 0x80"), "{msg}");
        assert!(msg.contains("(+2 more in this block)"), "{msg}");
    }
}

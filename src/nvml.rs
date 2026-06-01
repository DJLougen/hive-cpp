//! Hardware monitoring stubs. The Python `hive.hardware` module wraps
//! pynvml; the Rust port will use the `nvml` crate behind the
//! `nvidia-ml` feature flag. Step 2 ships the **shape**; Step 3 will
//! fill in the real samplers.

use std::time::Duration;

/// Sub-second power sampler. Currently a no-op. Step 3 will plug in
/// the real `nvml::device::Device` sampler.
#[derive(Debug, Default)]
pub struct PowerSampler {
    samples: Vec<(f64, f64, f64)>, // (t, power_mw, mem_mb)
}

impl PowerSampler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.samples.push((now, 0.0, 0.0));
    }

    pub fn energy_joules(&self) -> f64 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let mut total = 0.0_f64;
        for w in self.samples.windows(2) {
            let (t0, p0, _) = w[0];
            let (t1, p1, _) = w[1];
            total += (p0 + p1) / 2.0 * (t1 - t0) * 1e-3;
        }
        total
    }

    pub fn peak_memory_mb(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.2)
            .fold(0.0_f64, f64::max)
    }

    pub fn avg_power_w(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|s| s.1).sum();
        sum / self.samples.len() as f64 / 1000.0
    }
}

/// A `Duration` × `TDP` estimate — the same fallback the Python
/// `hive_benchmark.py` uses when NVML is unavailable.
pub fn estimate_cpu_energy_joules(elapsed: Duration, package_tdp_w: f32) -> f64 {
    let tdp = package_tdp_w as f64;
    let idle_fraction = 0.4; // matches the Python sibling
    elapsed.as_secs_f64() * tdp * idle_fraction
}

//! Per-operator quality metrics with sliding-window aggregation.
//!
//! Per §2.5, the SLA dimensions are: latency, throughput, completeness,
//! availability. The gateway needs a moving estimate of each to bias
//! routing decisions and to feed into telemetry.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct QualityWindow {
    pub window: Duration,
    samples: VecDeque<(Instant, f64)>,
}

impl QualityWindow {
    pub fn new(window: Duration) -> Self {
        Self { window, samples: VecDeque::new() }
    }

    pub fn record(&mut self, value: f64) {
        self.record_at(Instant::now(), value);
    }

    pub fn record_at(&mut self, now: Instant, value: f64) {
        self.samples.push_back((now, value));
        self.evict(now);
    }

    fn evict(&mut self, now: Instant) {
        while let Some(&(t, _)) = self.samples.front() {
            if now.duration_since(t) > self.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn average(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: f64 = self.samples.iter().map(|(_, v)| *v).sum();
        Some(sum / self.samples.len() as f64)
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }
}

#[derive(Debug, Clone)]
pub struct QualityMetrics {
    pub latency_ms: QualityWindow,
    pub throughput_blocks_per_s: QualityWindow,
    pub completeness_pct: QualityWindow,
    pub availability_pct: QualityWindow,
}

impl QualityMetrics {
    pub fn new(window: Duration) -> Self {
        Self {
            latency_ms: QualityWindow::new(window),
            throughput_blocks_per_s: QualityWindow::new(window),
            completeness_pct: QualityWindow::new(window),
            availability_pct: QualityWindow::new(window),
        }
    }

    /// Composite score in [0, 1] roughly, used to rank operators. Higher
    /// availability + completeness + throughput / lower latency.
    pub fn composite_score(&self) -> f64 {
        let availability = self.availability_pct.average().unwrap_or(100.0) / 100.0;
        let completeness = self.completeness_pct.average().unwrap_or(100.0) / 100.0;
        let throughput = self
            .throughput_blocks_per_s
            .average()
            .map(|t| (t / 1000.0).min(1.0))
            .unwrap_or(0.5);
        let latency_penalty = self
            .latency_ms
            .average()
            .map(|l| (1.0 - (l / 5_000.0).min(1.0)).max(0.0))
            .unwrap_or(0.5);
        availability * completeness * 0.5 + throughput * 0.2 + latency_penalty * 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_evicts_old_samples() {
        let mut w = QualityWindow::new(Duration::from_secs(10));
        let t0 = Instant::now();
        w.record_at(t0, 1.0);
        w.record_at(t0 + Duration::from_secs(5), 2.0);
        w.record_at(t0 + Duration::from_secs(12), 3.0);
        // Calling .average() now should evict the t0 sample on the next record.
        w.record_at(t0 + Duration::from_secs(13), 4.0);
        // Samples retained: at t0+5s, t0+12s, t0+13s — the first one is at age
        // 8s, still within the window. So count is 3, not 2.
        assert_eq!(w.count(), 3);
    }

    #[test]
    fn composite_score_higher_with_better_metrics() {
        let mut a = QualityMetrics::new(Duration::from_secs(60));
        a.latency_ms.record(100.0);
        a.throughput_blocks_per_s.record(500.0);
        a.completeness_pct.record(100.0);
        a.availability_pct.record(100.0);
        let mut b = QualityMetrics::new(Duration::from_secs(60));
        b.latency_ms.record(3_000.0);
        b.throughput_blocks_per_s.record(50.0);
        b.completeness_pct.record(80.0);
        b.availability_pct.record(95.0);
        assert!(a.composite_score() > b.composite_score());
    }
}

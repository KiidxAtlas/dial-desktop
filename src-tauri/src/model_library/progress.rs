//! Observe the official helper's stderr, not its private cache/download implementation. Never
//! retain raw logs (which can contain tokens, signed URLs, or unbounded terminal output).
use serde::Serialize;
use std::{
    io::Read,
    process::Command,
    sync::{Arc, Mutex},
};

pub(super) fn configure(cmd: &mut Command) {
    // hf 2.4.0 has two independent gates: CLI output mode can globally disable bars,
    // and Hub's tqdm wrapper auto-disables byte bars when stderr is not a TTY.
    cmd.env("HF_HUB_DISABLE_PROGRESS_BARS", "0")
        .env("TQDM_POSITION", "-1")
        // Standard HTTP and file-count bars respect this format. Xet and snapshot byte bars
        // supply their own explicit formats instead. No filenames in our protocol.
        .env("TQDM_BAR_FORMAT", "dial:{unit}:{n}:{total};");
}

#[derive(Default, Clone, Copy, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metrics {
    percent: Option<u8>,
    completed_bytes: Option<u64>, // logical file bytes, including resumed data
    total_bytes: Option<u64>,     // only when the denominator covers the entire selection
    received_bytes: Option<u64>,  // actual network transfer; Xet compression/dedup changes this
    completed_files: Option<u64>,
    total_files: Option<u64>,
}

#[derive(Default, Clone)]
pub(super) struct Report {
    single_file: bool,
    pub metrics: Metrics,
    pub recovery: Option<&'static str>,
}

/// hf's custom bars use tqdm's decimal SI scale and rounded values, e.g. `1.23GB`.
fn scaled_bytes(s: &str) -> Option<u64> {
    let s = s.strip_suffix('b')?;
    let (number, scale) = match s.as_bytes().last()? {
        b'k' => (&s[..s.len() - 1], 1e3),
        b'm' => (&s[..s.len() - 1], 1e6),
        b'g' => (&s[..s.len() - 1], 1e9),
        b't' => (&s[..s.len() - 1], 1e12),
        _ => (s, 1.0),
    };
    if number.len() > 12 || !number.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let bytes = number.parse::<f64>().ok()? * scale;
    (bytes < u64::MAX as f64).then(|| bytes.round() as u64)
}

impl Report {
    pub(super) fn new(single_file: bool) -> Self {
        Self {
            single_file,
            ..Self::default()
        }
    }

    fn observe_progress(&mut self, line: &str) {
        if let Some(record) = line
            .strip_prefix("dial:")
            .and_then(|s| s.split_once(';'))
            .map(|(s, _)| s)
        {
            let mut fields = record.split(':');
            let unit = fields.next();
            let Some(n) = fields.next().and_then(|s| s.parse::<u64>().ok()) else {
                return;
            };
            let total = match fields.next() {
                Some("none") => None,
                Some(s) => match s.parse::<u64>() {
                    Ok(t) => Some(t),
                    Err(_) => return,
                },
                None => return,
            };
            if fields.next().is_some() {
                return;
            }
            match unit {
                Some("b") if self.single_file => {
                    self.metrics.completed_bytes = Some(n);
                    self.metrics.total_bytes = total.filter(|&t| t > 0);
                    self.metrics.percent = self
                        .metrics
                        .total_bytes
                        .map(|t| ((u128::from(n) * 100 / u128::from(t)).min(100)) as u8);
                }
                Some("it") if !self.single_file && total.is_some_and(|t| n <= t) => {
                    self.metrics.completed_files = Some(n);
                    self.metrics.total_files = total;
                }
                _ => {}
            }
            return;
        }

        // Xet's second bar measures network traffic, not completed/reconstructed file bytes.
        // Snapshot bars aggregate across files, but the reconstruction total only includes files
        // discovered *so far*. It can show 100% while other shards haven't even started.
        let Some((bar, details)) = line.rsplit_once('|') else {
            return;
        };
        let Some((desc, _)) = bar.rsplit_once(':') else {
            return;
        };
        let mut parts = details
            .split(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | '\x1b'))
            .filter(|s| !s.is_empty());
        let Some(n) = parts.next().and_then(scaled_bytes) else {
            return;
        };
        let network = if self.single_file {
            desc.ends_with(": downloading bytes")
        } else {
            matches!(desc, "downloading bytes" | "download complete")
        };
        if network {
            self.metrics.received_bytes = Some(n);
            return;
        }
        let reconstruction = if self.single_file {
            desc.ends_with(": reconstructing file")
        } else {
            matches!(
                desc,
                "reconstructing (incomplete total...)" | "reconstruction complete"
            )
        };
        if reconstruction && parts.next() == Some("/") {
            self.metrics.completed_bytes = Some(n);
            if self.single_file
                && let Some(total) = parts.next().and_then(scaled_bytes).filter(|&t| t > 0)
            {
                self.metrics.total_bytes = Some(total);
                self.metrics.percent = bar
                    .split_once("%|")
                    .and_then(|(prefix, _)| {
                        prefix.split_whitespace().next_back()?.parse::<u8>().ok()
                    })
                    .filter(|&p| p <= 100);
            }
        }
    }

    fn observe(&mut self, bytes: &[u8]) {
        let line = String::from_utf8_lossy(bytes).to_ascii_lowercase();
        self.observe_progress(line.trim());
        let recovery = if line.contains("no space left")
            || line.contains("disk full")
            || line.contains("not enough space")
        {
            "Free disk space in the Hugging Face cache, then retry."
        } else if line.contains("gatedrepo")
            || line.contains("401 unauthorized")
            || line.contains("403 forbidden")
            || line.contains("gated repo")
        {
            "Check access to this model and your Hugging Face sign-in, then retry."
        } else if line.contains("permission denied") || line.contains("access is denied") {
            "Check write permission for your Hugging Face cache, then retry."
        } else if line.contains("timed out")
            || line.contains("connection error")
            || line.contains("connection refused")
        {
            "Check your network connection, then retry."
        } else {
            return;
        };
        self.recovery = Some(recovery);
    }
}

pub(super) fn drain(mut reader: impl Read, report: &Arc<Mutex<Report>>) {
    let mut buffer = [0; 4096];
    let mut line = Vec::new();
    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 {
            break;
        }
        let mut report = report.lock().unwrap_or_else(|e| e.into_inner());
        for &b in &buffer[..n] {
            if matches!(b, b'\n' | b'\r') {
                if !line.is_empty() {
                    report.observe(&line);
                }
                line.clear();
            } else if line.len() < 2048 {
                line.push(b);
            }
        }
        // tqdm writes CR *before* the next update. Don't wait for another record (or EOF)
        // to publish the last flushed counter. Delimited fields ignore partial pipe writes.
        if !line.is_empty() {
            report.observe(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_safe_transfer_details_across_terminal_records() {
        let report = Arc::new(Mutex::new(Report::new(true)));
        let mut stderr = vec![b'x'; 24_000];
        stderr.extend_from_slice(b"\rmodel: reconstructing file:  51%|#####     | 2.45MB / 4.81MB, 1.33MB/s  \x1b[A\rmodel: downloading bytes: ####| 1.23MB, 1.14MB/s\rmodel: downloading bytes: ####| NaNGB\nNo space left on device: secret_url");
        drain(&stderr[..], &report);
        let r = report.lock().unwrap();
        assert_eq!(
            r.metrics,
            Metrics {
                percent: Some(51),
                completed_bytes: Some(2_450_000),
                total_bytes: Some(4_810_000),
                received_bytes: Some(1_230_000),
                ..Default::default()
            }
        );
        assert!(r.recovery.unwrap().starts_with("Free disk space"));
    }

    #[test]
    fn snapshot_reports_aggregate_counters_but_not_incomplete_or_per_file_percentages() {
        let report = Arc::new(Mutex::new(Report::default()));
        let stderr = b"\rDownloading bytes: #####| 5.21MB\n\rReconstructing (incomplete total...): 100%|#####| 10.5MB / 10.5MB\x1b[A\rdial:it:1:3;\rdial:B:100:100;\rother.gguf: downloading bytes: #| 42GB\rother.gguf: reconstructing file: 42%|#| 42GB / 100GB";
        drain(&stderr[..], &report);
        assert_eq!(
            report.lock().unwrap().metrics,
            Metrics {
                completed_bytes: Some(10_500_000),
                received_bytes: Some(5_210_000),
                completed_files: Some(1),
                total_files: Some(3),
                ..Default::default()
            }
        );
    }

    #[test]
    fn single_file_progress_does_not_require_a_known_size_or_next_record() {
        let report = Arc::new(Mutex::new(Report::new(true)));
        for (record, n, total, percent) in [
            ("dial:B:424242:None;", 424242, None, None),
            ("dial:B:424243:1000003;", 424243, Some(1000003), Some(42)),
        ] {
            // Pipe chunks may split a numeric field; don't report the partial value.
            struct BeforeEof<'a>(&'a Mutex<Report>, u64);
            impl Read for BeforeEof<'_> {
                fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                    // The counter must already be visible before the reader learns of EOF.
                    assert_eq!(self.0.lock().unwrap().metrics.completed_bytes, Some(self.1));
                    Ok(0)
                }
            }
            let (a, b) = record.as_bytes().split_at(12);
            drain(a.chain(b).chain(BeforeEof(&report, n)), &report);
            assert_eq!(
                report.lock().unwrap().metrics,
                Metrics {
                    percent,
                    completed_bytes: Some(n),
                    total_bytes: total,
                    ..Default::default()
                }
            );
        }
    }

    /// Opt-in, offline smoke: set DIAL_HF_TEST_PYTHON to the absolute interpreter with the
    /// pinned huggingface_hub installed. Exercise real Xet/CLI/tqdm code with synthetic counts,
    /// no authentication, remote model, actual cache or TTY.
    #[test]
    #[ignore = "requires Python with huggingface_hub 2.4.0; never installs or downloads"]
    fn pinned_helper_reports_xet_bytes_through_a_pipe() {
        let temp = tempfile::tempdir().unwrap();
        let mut cmd =
            Command::new(std::env::var_os("DIAL_HF_TEST_PYTHON").expect("set DIAL_HF_TEST_PYTHON"));
        cmd.env_clear().current_dir(temp.path());
        if let Some(root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", root);
        }
        for key in [
            "HOME",
            "USERPROFILE",
            "HF_HOME",
            "XDG_CACHE_HOME",
            "TMPDIR",
            "TEMP",
            "TMP",
        ] {
            cmd.env(key, temp.path());
        }
        configure(&mut cmd);
        cmd.env("HF_HUB_OFFLINE", "1").env("HF_HUB_DISABLE_TELEMETRY", "1").args(["-I", "-B", "-c", r#"
import huggingface_hub, sys
from types import SimpleNamespace
from huggingface_hub.cli._output import Output, OutputFormat
from huggingface_hub.utils._xet_progress_reporting import XetDownloadProgressReporter
from huggingface_hub.utils.tqdm import tqdm
assert huggingface_hub.__version__ == '2.4.0' and not sys.stderr.isatty()
Output().set_mode(OutputFormat.agent)
with XetDownloadProgressReporter(reconstruction_desc='test: reconstructing file', transfer_desc='test: downloading bytes', total=4810000, log_level=30, name='test') as x:
    x.update_progress(SimpleNamespace(total_bytes_completed=2450000, total_transfer_bytes_completed=1230000, total_bytes=4810000, total_bytes_completion_rate=0, total_transfer_bytes_completion_rate=0))
with tqdm(desc='Fetching 3 files', total=3) as files: files.update(1)
"#]);
        let (exit, _, stderr) =
            super::super::run_bounded(cmd, std::time::Duration::from_secs(20)).unwrap();
        assert!(exit.success(), "{}", String::from_utf8_lossy(&stderr));
        for single in [true, false] {
            let report = Arc::new(Mutex::new(Report::new(single)));
            drain(&stderr[..], &report);
            let metrics = report.lock().unwrap().metrics;
            if single {
                assert_eq!(
                    (
                        metrics.received_bytes,
                        metrics.completed_bytes,
                        metrics.percent
                    ),
                    (Some(1_230_000), Some(2_450_000), Some(51))
                );
            } else {
                assert_eq!(
                    (metrics.completed_files, metrics.total_files),
                    (Some(1), Some(3))
                );
            }
        }
    }
}

use crate::utils::tty;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::Command;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) type SharedDownloadProgress = Arc<Mutex<DownloadProgress>>;

#[derive(Clone)]
pub(crate) struct DownloadProgress {
    message: String,
    phase: &'static str,
    fetched_size: u64,
    total_size: Option<u64>,
    done: bool,
}

impl DownloadProgress {
    fn new(message: String) -> Self {
        Self {
            message,
            phase: "downloading",
            fetched_size: 0,
            total_size: None,
            done: false,
        }
    }
}

pub(crate) fn new_download_progress(message: String) -> SharedDownloadProgress {
    Arc::new(Mutex::new(DownloadProgress::new(message)))
}

pub(crate) fn download_progress_enabled() -> bool {
    io::stdout().is_terminal() && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

pub(crate) fn should_render_progress(tty_with_cursor_support: bool) -> bool {
    tty_with_cursor_support
}

pub(crate) fn update_download_total(progress: &SharedDownloadProgress, total_size: Option<u64>) {
    if let Ok(mut progress) = progress.lock() {
        progress.total_size = total_size;
        progress.fetched_size = 0;
    }
}

pub(crate) fn update_download_phase(progress: &SharedDownloadProgress, phase: &'static str) {
    if let Ok(mut progress) = progress.lock() {
        progress.phase = phase;
    }
}

pub(crate) fn increment_downloaded_size(progress: &SharedDownloadProgress, bytes: u64) {
    if let Ok(mut progress) = progress.lock() {
        progress.fetched_size += bytes;
    }
}

pub(crate) fn mark_download_complete(progress: &SharedDownloadProgress) {
    if let Ok(mut progress) = progress.lock() {
        progress.done = true;
    }
}

pub(crate) struct ProgressRenderer {
    active: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

impl ProgressRenderer {
    pub(crate) fn start(progress: &[SharedDownloadProgress]) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let progress = progress.to_vec();
        let thread_active = Arc::clone(&active);

        let handle = thread::spawn(move || {
            let mut stdout = io::stdout();
            let mut spinner = Spinner::new();
            let mut initial_render = true;
            let terminal_width = terminal_width();
            let message_length_max = progress
                .iter()
                .filter_map(|entry| entry.lock().ok().map(|entry| entry.message.len()))
                .max()
                .unwrap_or(0);
            let _ = write!(stdout, "{}", tty::hide_cursor());
            let mut rendered_lines = 0;
            let mut printed_done = vec![false; progress.len()];

            loop {
                if initial_render {
                    thread::sleep(Duration::from_millis(50));
                    initial_render = false;
                }

                clear_rendered_lines(&mut stdout, rendered_lines);

                let snapshots = progress
                    .iter()
                    .filter_map(|entry| entry.lock().ok().map(|entry| entry.clone()))
                    .collect::<Vec<_>>();
                let pending_lines = snapshots
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        if entry.done {
                            if !printed_done[index] {
                                let _ = writeln!(
                                    stdout,
                                    "{green}✔︎{reset} {message}",
                                    green = tty::green(),
                                    reset = tty::reset(),
                                    message = entry.message
                                );
                                printed_done[index] = true;
                            }
                            return None;
                        }

                        Some(format!(
                            "{}{}{} {}",
                            tty::blue(),
                            spinner.frame(),
                            tty::reset(),
                            format_progress_message(
                                &entry.message,
                                entry.phase,
                                entry.fetched_size,
                                entry.total_size,
                                terminal_width,
                                message_length_max,
                            )
                        ))
                    })
                    .collect::<Vec<_>>();

                rendered_lines = pending_lines.len();
                for (index, line) in pending_lines.iter().enumerate() {
                    let _ = write!(stdout, "{line}{clear}", clear = tty::clear_to_end());
                    if index + 1 < pending_lines.len() {
                        let _ = writeln!(stdout);
                    }
                }
                let _ = stdout.flush();

                if !thread_active.load(Ordering::Relaxed) {
                    break;
                }

                if rendered_lines == 1 {
                    let _ = write!(stdout, "\r");
                } else if rendered_lines > 1 {
                    let _ = write!(stdout, "{}\r", tty::move_cursor_up(rendered_lines - 1));
                }

                thread::sleep(Duration::from_millis(50));
            }

            clear_rendered_lines(&mut stdout, rendered_lines);
            let _ = write!(stdout, "{}", tty::show_cursor());
            let _ = stdout.flush();
        });

        Self { active, handle }
    }

    pub(crate) fn stop(self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<usize>().ok())
        .or_else(|| {
            tty_command_output("/bin/stty size </dev/tty 2>/dev/null").and_then(|size| {
                size.split_whitespace()
                    .nth(1)
                    .and_then(|width| width.parse::<usize>().ok())
            })
        })
        .or_else(|| {
            tty_command_output("/usr/bin/tput cols 2>/dev/null")
                .and_then(|width| width.parse::<usize>().ok())
        })
        .filter(|columns| *columns > 0)
        .unwrap_or(80)
}

fn tty_command_output(command: &str) -> Option<String> {
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let output = String::from_utf8(output.stdout).ok()?;
    let output = output.trim();
    if output.is_empty() {
        return None;
    }

    Some(output.to_string())
}

fn format_progress_message(
    message: &str,
    phase: &str,
    fetched_size: u64,
    total_size: Option<u64>,
    width: usize,
    message_length_max: usize,
) -> String {
    let available_width = width.saturating_sub(2);
    let formatted_fetched_size = format_disk_usage_readable_size(fetched_size);
    let formatted_total_size = total_size
        .map(format_disk_usage_readable_size)
        .unwrap_or_else(|| "-".repeat(7));
    let mut progress = format!(
        " {:phase_width$} {formatted_fetched_size}/{formatted_total_size}",
        capitalize_phase(phase),
        phase_width = 11,
    );
    let bar_length = 4
        .max(available_width.saturating_sub(progress.len() + message_length_max.saturating_add(1)));

    if phase == "downloading" && total_size.is_some() {
        let total_size = total_size.unwrap_or(1);
        let percent = (fetched_size as f64 / total_size.max(1) as f64).clamp(0.0, 1.0);
        let used = ((percent * bar_length as f64).round() as usize).min(bar_length);
        progress = format!(
            " {}{}{}",
            "#".repeat(used),
            " ".repeat(bar_length - used),
            progress
        );
    }

    let message_width = available_width.saturating_sub(progress.len());
    let mut truncated_message = message.chars().take(message_width).collect::<String>();
    let padding = message_width.saturating_sub(truncated_message.chars().count());
    truncated_message.push_str(&" ".repeat(padding));

    format!("{truncated_message}{progress}")
}

fn capitalize_phase(phase: &str) -> String {
    let mut characters = phase.chars();
    match characters.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), characters.as_str()),
        None => String::new(),
    }
}

fn format_disk_usage_readable_size(size_in_bytes: u64) -> String {
    let (size, unit) = disk_usage_readable_size_unit(size_in_bytes as f64);
    format!("{size:>5.1}{unit:>2}")
}

fn disk_usage_readable_size_unit(size_in_bytes: f64) -> (f64, &'static str) {
    let mut size = size_in_bytes;
    let mut unit = "B";

    for next_unit in ["KB", "MB", "GB"] {
        if round_to_precision(size, 1) < 1000.0 {
            break;
        }

        size /= 1000.0;
        unit = next_unit;
    }

    (size, unit)
}

fn round_to_precision(value: f64, precision: u32) -> f64 {
    let multiplier = 10_f64.powi(precision as i32);
    (value * multiplier).round() / multiplier
}

fn clear_rendered_lines(stdout: &mut impl Write, lines: usize) {
    if lines == 0 {
        return;
    }

    let _ = write!(stdout, "\r");
    if lines > 1 {
        let _ = write!(stdout, "{}", tty::move_cursor_up(lines - 1));
    }

    for index in 0..lines {
        let _ = write!(stdout, "{}", tty::clear_entire_line());
        if index + 1 < lines {
            let _ = writeln!(stdout);
        }
    }

    if lines > 1 {
        let _ = write!(stdout, "{}", tty::move_cursor_up(lines - 1));
    }
    let _ = write!(stdout, "\r");
}

struct Spinner {
    start: Instant,
    index: usize,
}

impl Spinner {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            index: 0,
        }
    }

    fn frame(&mut self) -> &'static str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠚", "⠞", "⠖", "⠦", "⠴", "⠲", "⠳", "⠓"];

        if self.start.elapsed() >= Duration::from_millis(100) {
            self.start = Instant::now();
            self.index = (self.index + 1) % FRAMES.len();
        }

        FRAMES[self.index]
    }
}

#[cfg(test)]
mod tests {
    use super::{format_disk_usage_readable_size, format_progress_message, should_render_progress};

    #[test]
    fn matches_homebrews_disk_usage_alignment() {
        assert_eq!(format_disk_usage_readable_size(0), "  0.0 B");
        assert_eq!(format_disk_usage_readable_size(11_700_000), " 11.7MB");
    }

    #[test]
    fn formats_progress_lines_like_homebrews_download_queue() {
        assert_eq!(
            format_progress_message(
                "Bottle ffmpeg (8.1)",
                "downloading",
                11_000_000,
                Some(11_700_000),
                80,
                19,
            ),
            "Bottle ffmpeg (8.1) ############################   Downloading  11.0MB/ 11.7MB"
        );
    }

    #[test]
    fn progress_messages_fill_the_available_width() {
        assert_eq!(
            format_progress_message(
                "Bottle ffmpeg (8.1)",
                "downloading",
                11_000_000,
                Some(11_700_000),
                120,
                19,
            )
            .chars()
            .count(),
            118
        );
    }

    #[test]
    fn renders_live_progress_for_single_downloads_when_tty_support_is_available() {
        assert!(should_render_progress(true));
        assert!(!should_render_progress(false));
    }
}

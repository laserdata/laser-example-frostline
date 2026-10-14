use serde::Serialize;
use std::fs;
use std::io;
use std::process::Command;

const KIB: u64 = 1024;
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
// utime and stime are fields 14 and 15 of /proc/<pid>/stat. Counting from the field after the name, they sit at 11 and 12.
const UTIME_AFTER_NAME: usize = 11;
const STIME_AFTER_NAME: usize = 12;

/// CPU time and memory of one process at one moment, read from /proc.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ProcessSample {
    pub cpu_ticks: u64,
    pub start_ticks: u64,
    pub rss_bytes: u64,
    pub peak_rss_bytes: u64,
}

pub fn read(pid: u32) -> Option<ProcessSample> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    Some(ProcessSample {
        cpu_ticks: cpu_ticks(&stat)?,
        start_ticks: start_ticks(&stat)?,
        rss_bytes: status_kib(&status, "VmRSS:")? * KIB,
        peak_rss_bytes: status_kib(&status, "VmHWM:")? * KIB,
    })
}

/// Proportional set size from /proc/<pid>/smaps_rollup, which splits shared pages between their users.
pub fn pss_bytes(pid: u32) -> Option<u64> {
    let rollup = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    Some(status_kib(&rollup, "Pss:")? * KIB)
}

/// The command name can hold spaces and parentheses, so fields count from its last closing parenthesis.
pub fn cpu_ticks(stat: &str) -> Option<u64> {
    let fields: Vec<&str> = stat[stat.rfind(')')? + 1..].split_whitespace().collect();
    let utime: u64 = fields.get(UTIME_AFTER_NAME)?.parse().ok()?;
    let stime: u64 = fields.get(STIME_AFTER_NAME)?.parse().ok()?;
    Some(utime + stime)
}

pub fn start_ticks(stat: &str) -> Option<u64> {
    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

pub fn status_kib(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
}

/// The memory accounting of the cgroup a process runs in, read from its cgroup v2 files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct CgroupMemory {
    pub peak_bytes: u64,
    /// `None` when the cgroup has no memory limit.
    pub max_bytes: Option<u64>,
    pub oom_kills: u64,
}

pub fn cgroup_memory(pid: u32) -> Option<CgroupMemory> {
    let membership = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))?;
    let directory = format!("{CGROUP_ROOT}{path}");
    let peak_bytes = fs::read_to_string(format!("{directory}/memory.peak"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let max_bytes = fs::read_to_string(format!("{directory}/memory.max"))
        .ok()?
        .trim()
        .parse()
        .ok();
    let events = fs::read_to_string(format!("{directory}/memory.events")).ok()?;
    Some(CgroupMemory {
        peak_bytes,
        max_bytes,
        oom_kills: event_count(&events, "oom_kill"),
    })
}

pub fn event_count(events: &str, key: &str) -> u64 {
    events
        .lines()
        .find_map(|line| {
            line.strip_prefix(key)?
                .strip_prefix(' ')?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

/// Clock ticks per second, the unit of the CPU fields in /proc.
pub fn ticks_per_second() -> io::Result<u64> {
    let output = Command::new("getconf").arg("CLK_TCK").output()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| io::Error::other("getconf CLK_TCK did not print a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_stat_line_with_spaces_in_the_name_when_parsed_then_should_sum_user_and_system_ticks()
    {
        let stat = "4242 (frostline (x) y) S 1 4242 4242 0 -1 4194304 1200 0 0 0 731 112 0 0 20 0 9 0 5000 1000000 2500";
        assert_eq!(cpu_ticks(stat), Some(843));
    }

    #[test]
    fn given_a_stat_line_when_parsed_then_should_read_the_process_identity() {
        let stat = "4242 (frostline (x) y) S 1 4242 4242 0 -1 4194304 1200 0 0 0 731 112 0 0 20 0 9 0 5000 1000000 2500";
        assert_eq!(start_ticks(stat), Some(5000));
        assert_eq!(start_ticks("broken"), None);
    }

    #[test]
    fn given_memory_events_when_parsed_then_should_read_the_oom_kill_count() {
        let events = "low 0\nhigh 0\nmax 3\noom 1\noom_kill 1\noom_group_kill 0\n";
        assert_eq!(
            (
                event_count(events, "oom_kill"),
                event_count(events, "oom"),
                event_count(events, "absent")
            ),
            (1, 1, 0)
        );
    }

    #[test]
    fn given_status_and_rollup_text_when_parsed_then_should_read_kibibyte_fields() {
        let status = "Name:\tfrostline\nVmHWM:\t   81234 kB\nVmRSS:\t   80000 kB\n";
        assert_eq!(
            (status_kib(status, "VmHWM:"), status_kib(status, "VmRSS:")),
            (Some(81_234), Some(80_000))
        );
        assert_eq!(status_kib("Rss: 10 kB\nPss:  7 kB\n", "Pss:"), Some(7));
        assert_eq!(status_kib(status, "VmSwap:"), None);
    }
}

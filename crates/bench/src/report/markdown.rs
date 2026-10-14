use super::{BenchResult, GroupResult, SCOPE, SERVER};
use frostline_shared::measure::{ByteSize, Reduction};

impl BenchResult {
    pub fn markdown(&self) -> String {
        let mut text = format!(
            "# Frostline bench {}\n\n{}. {} repetitions, each a fresh run of the same seeded dataset: {} records, {} of payload. Every repetition selected the same records with the same payload bytes.\n\n```sh\n{}\n```\n\n",
            self.profile,
            self.description,
            self.repetitions.len(),
            self.source_records,
            ByteSize::from(self.source_bytes),
            self.reproduce
        );
        text.push_str("| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for group in &self.groups {
            text.push_str(&format!(
                "| {} | {} | {} | {} | **{}** | {} | {} | **{}** |\n",
                group.group,
                group.matches,
                ByteSize::from(group.payload_bytes),
                ByteSize::from(group.payload_full_feed_bytes),
                group.payload_reduction,
                ByteSize::from(group.wire_bytes),
                ByteSize::from(group.wire_full_feed_bytes),
                group.wire_reduction
            ));
        }
        let sum = |pick: fn(&GroupResult) -> u64| self.groups.iter().map(pick).sum::<u64>();
        let (payload, payload_full, wire, wire_full) = (
            sum(|group| group.payload_bytes),
            sum(|group| group.payload_full_feed_bytes),
            sum(|group| group.wire_bytes),
            sum(|group| group.wire_full_feed_bytes),
        );
        text.push_str(&format!(
            "| all groups | | {} | {} | **{}** | {} | {} | **{}** |\n\n{SCOPE}\n\n",
            ByteSize::from(payload),
            ByteSize::from(payload_full),
            Reduction::compute(payload_full, payload),
            ByteSize::from(wire),
            ByteSize::from(wire_full),
            Reduction::compute(wire_full, wire)
        ));
        text.push_str("| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |\n| --- | --- | --- | --- | --- | --- | --- |\n");
        for group in &self.groups {
            text.push_str(&format!(
                "| {} | {:.2} s | {:.2} s | {} | {} | {} | {} |\n",
                group.group,
                group.cpu_seconds,
                group.full_feed_cpu_seconds,
                ByteSize::from(group.peak_rss_bytes),
                ByteSize::from(group.full_feed_peak_rss_bytes),
                group.connections,
                group.full_feed_connections
            ));
        }
        text.push_str("\n| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for group in &self.groups {
            let (filtered, full) = (&group.fetch_latency, &group.full_feed_fetch_latency);
            text.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                group.group,
                micros(filtered.p50_us),
                micros(filtered.p99_us),
                micros(filtered.p999_us),
                filtered.fetches,
                micros(full.p50_us),
                micros(full.p99_us),
                micros(full.p999_us),
                full.fetches
            ));
        }
        text.push_str("\nFiltered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.\n");
        let throughput = &self.throughput;
        text.push_str(&format!(
            "\nPublishing ran at {:.0} records per second. The filtered read took {:.2} s and {:.2} s of {SERVER} CPU, with {SERVER} at {} peak RSS. The full-feed read took {:.2} s and {:.2} s of {SERVER} CPU, at {} peak RSS. {SERVER} examined {:.0} records per CPU second while filtering.\n\n| binary | version |\n| --- | --- |\n",
            throughput.publish_records_per_second, throughput.filtered_wall_seconds, throughput.filtered_server_cpu_seconds,
            ByteSize::from(throughput.filtered_server_peak_rss_bytes),
            throughput.full_feed_wall_seconds, throughput.full_feed_server_cpu_seconds,
            ByteSize::from(throughput.full_feed_server_peak_rss_bytes),
            throughput.examined_records_per_server_cpu_second
        ));
        for binary in &self.binaries {
            text.push_str(&format!("| {} | `{}` |\n", binary.name, binary.version));
        }
        text
    }
}

fn micros(value: u32) -> String {
    if value >= 1000 {
        format!("{:.2} ms", f64::from(value) / 1000.0)
    } else {
        format!("{value} us")
    }
}

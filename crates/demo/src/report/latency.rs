use super::ReaderLine;

pub fn append(text: &mut String, readers: &[ReaderLine]) {
    text.push_str("\n## Reader latency\n\nFiltered measurements cover one bounded reader round, including empty pages and progress work. Ordinary measurements cover one native batch poll. These are distinct operations. Compare their examined counts and scan limits before comparing percentiles.\n\n| Group | Worker | Operation | Calls | p50 | p99 | p99.9 | Examined per call | Error bound |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for reader in readers {
        let latency = reader.summary.fetch_latency;
        text.push_str(&format!(
            "| {} | {} | {} | {} | {:.3} ms | {:.3} ms | {:.3} ms | {} | {:.3}% |\n",
            reader.group,
            reader.worker,
            if reader.baseline {
                "ordinary poll"
            } else {
                "filtered round"
            },
            latency.fetches,
            f64::from(latency.p50_us) / 1000.0,
            f64::from(latency.p99_us) / 1000.0,
            f64::from(latency.p999_us) / 1000.0,
            latency.examined_per_fetch,
            f64::from(latency.relative_error_ppm) / 10_000.0,
        ));
    }
    text.push_str("\nWith fewer than 1,000 calls, p99.9 resolves to the maximum. Longer runs switch from exact samples to a fixed histogram and report its error bound. These observations do not establish a production latency guarantee.\n");
}

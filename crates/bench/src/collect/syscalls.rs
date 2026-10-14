use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

/// strace options that log every successful socket read and write with its resolved TCP endpoints and no data.
pub const STRACE_ARGUMENTS: [&str; 9] = [
    "-f",
    "-qq",
    "-s0",
    "-yy",
    "-e",
    "trace=read,readv,recvfrom,recvmsg,write,writev,sendto,sendmsg",
    "-e",
    "status=successful",
    "-o",
];
const RECEIVING: [&str; 4] = ["read", "readv", "recvfrom", "recvmsg"];
const UNFINISHED: &str = "<unfinished ...>";

/// Bytes one process moved through its TCP sockets, summed from the return value of every socket syscall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct WireUsage {
    pub received_bytes: u64,
    pub sent_bytes: u64,
    pub connections: u32,
}

pub fn read(path: &Path) -> io::Result<WireUsage> {
    let mut tally = Tally::default();
    for line in BufReader::new(File::open(path)?).lines() {
        tally.line(&line?);
    }
    Ok(tally.finish())
}

/// A multi-threaded trace can split one call into an unfinished and a resumed line, so the start is kept per thread.
#[derive(Debug, Default)]
pub struct Tally {
    usage: WireUsage,
    connections: BTreeSet<String>,
    unfinished: HashMap<String, (String, Option<String>)>,
}

impl Tally {
    pub fn line(&mut self, line: &str) {
        let Some((thread, rest)) = line.split_once(' ') else {
            return;
        };
        let rest = rest.trim_start();
        if let Some(resumed) = rest.strip_prefix("<... ") {
            if let Some((call, endpoint)) = self.unfinished.remove(thread) {
                self.count(&call, endpoint, returned(resumed));
            }
            return;
        }
        let Some((call, arguments)) = rest.split_once('(') else {
            return;
        };
        let endpoint = tcp_endpoint(arguments);
        if rest.ends_with(UNFINISHED) {
            self.unfinished
                .insert(thread.to_owned(), (call.to_owned(), endpoint));
            return;
        }
        self.count(call, endpoint, returned(rest));
    }

    pub fn finish(mut self) -> WireUsage {
        self.usage.connections = u32::try_from(self.connections.len()).unwrap_or(u32::MAX);
        self.usage
    }

    fn count(&mut self, call: &str, endpoint: Option<String>, bytes: Option<u64>) {
        let (Some(endpoint), Some(bytes)) = (endpoint, bytes) else {
            return;
        };
        self.connections.insert(endpoint);
        if RECEIVING.contains(&call) {
            self.usage.received_bytes += bytes;
        } else {
            self.usage.sent_bytes += bytes;
        }
    }
}

// The first argument of a TCP call reads `9<TCP:[127.0.0.1:47552->127.0.0.1:8090]>`, or TCPv6 for IPv6.
fn tcp_endpoint(arguments: &str) -> Option<String> {
    let start = arguments.find("<TCP")?;
    let annotation = &arguments[start..];
    let open = annotation.find('[')?;
    let close = annotation.find(']')?;
    Some(annotation[open + 1..close].to_owned())
}

fn returned(line: &str) -> Option<u64> {
    line.rsplit_once(" = ")?
        .1
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE: &str = "2385273 sendto(9<TCP:[127.0.0.1:47552->127.0.0.1:8090]>, \"\"..., 291, MSG_NOSIGNAL, NULL, 0) = 291
2385243 recvfrom(9<TCP:[127.0.0.1:47552->127.0.0.1:8090]>, \"\"..., 8192, 0, NULL, NULL) = 285
2385243 recvfrom(11<TCP:[127.0.0.1:47600->127.0.0.1:8090]>, <unfinished ...>
2385244 write(1</tmp/reader.log>, \"\"..., 40) = 40
2385243 <... recvfrom resumed>\"\"..., 8192, 0, NULL, NULL) = 8192
2385244 write(4<anon_inode:[eventfd]>, \"\"..., 8) = 8
2385250 recvfrom(12<UNIX-STREAM:[17675100]>, \"\"..., 64, 0, NULL, NULL) = 64
";

    #[test]
    fn given_a_trace_with_split_calls_when_tallied_then_should_count_only_tcp_bytes() {
        let mut tally = Tally::default();
        for line in TRACE.lines() {
            tally.line(line);
        }
        assert_eq!(
            tally.finish(),
            WireUsage {
                received_bytes: 285 + 8192,
                sent_bytes: 291,
                connections: 2
            }
        );
    }
}

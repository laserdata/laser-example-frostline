use std::io;
use tokio::process::Command;

/// The counters of one TCP socket and the processes that hold it, from one `ss` run.
#[derive(Clone, Debug, PartialEq)]
pub struct SocketLine {
    pub pids: Vec<u32>,
    pub local: String,
    pub peer: String,
    pub received_bytes: u64,
    pub acked_bytes: u64,
}

/// Every TCP socket of this user with kernel counters, as `ss -tinpH` prints them.
pub async fn snapshot() -> io::Result<Vec<SocketLine>> {
    let output = Command::new("ss").arg("-tinpH").output().await?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "ss exited with {}",
            output.status
        )));
    }
    Ok(parse(&String::from_utf8_lossy(&output.stdout)))
}

/// A socket is one unindented line with its addresses and owners, then one indented line with its counters.
pub fn parse(output: &str) -> Vec<SocketLine> {
    let mut sockets: Vec<SocketLine> = Vec::new();
    for line in output.lines() {
        if line.starts_with(char::is_whitespace) {
            if let Some(socket) = sockets.last_mut() {
                socket.received_bytes = counter(line, "bytes_received:");
                socket.acked_bytes = counter(line, "bytes_acked:");
            }
            continue;
        }
        let columns: Vec<&str> = line.split_whitespace().collect();
        let (Some(local), Some(peer)) = (columns.get(3), columns.get(4)) else {
            continue;
        };
        sockets.push(SocketLine {
            pids: owners(line),
            local: (*local).to_owned(),
            peer: (*peer).to_owned(),
            received_bytes: 0,
            acked_bytes: 0,
        });
    }
    sockets
}

// ss leaves a counter out while it is zero.
fn counter(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .find_map(|field| field.strip_prefix(key))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn owners(line: &str) -> Vec<u32> {
    line.split("pid=")
        .skip(1)
        .filter_map(|rest| {
            rest.split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .filter_map(|digits| digits.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "ESTAB 0      0          127.0.0.1:58510        127.0.0.1:8090 users:((\"frostline-consu\",pid=4242,fd=41))
\t cubic wscale:10,10 rto:212 mss:65483 bytes_sent:1996069 bytes_acked:1996069 bytes_received:7445224 segs_out:6114
ESTAB 0      0          127.0.0.1:8090        127.0.0.1:58510 users:((\"iggy-server\",pid=77,fd=12),(\"iggy-server\",pid=78,fd=12))
\t cubic wscale:10,10 rto:212 bytes_sent:7445224 bytes_acked:7445224 segs_out:9
";

    #[test]
    fn given_ss_output_when_parsed_then_should_read_owners_addresses_and_counters() {
        let sockets = parse(OUTPUT);
        assert_eq!(sockets.len(), 2);
        assert_eq!(sockets[0].pids, [4242]);
        assert_eq!(
            (sockets[0].local.as_str(), sockets[0].peer.as_str()),
            ("127.0.0.1:58510", "127.0.0.1:8090")
        );
        assert_eq!(
            (sockets[0].received_bytes, sockets[0].acked_bytes),
            (7_445_224, 1_996_069)
        );
        assert_eq!(sockets[1].pids, [77, 78]);
        assert_eq!(sockets[1].received_bytes, 0);
    }
}

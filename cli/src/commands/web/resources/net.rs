//! `/proc/net/dev` parsing: total rx/tx bytes across real interfaces
//! (loopback excluded — it isn't network throughput).

/// Summed rx/tx byte counters across all non-loopback interfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Parse `/proc/net/dev`. Format is a two-line header followed by
/// `iface: rx_bytes rx_packets ... tx_bytes tx_packets ...` (rx has 8 fields,
/// tx starts at field 9). `lo` is skipped; docker's `veth*`/`docker0`/`br-*`
/// legitimately count (container traffic still crosses the host).
pub fn parse_proc_net_dev(input: &str) -> NetCounters {
    let mut total = NetCounters::default();
    for line in input.lines() {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface.trim();
        if iface.is_empty() || iface == "lo" {
            continue;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 9 {
            continue;
        }
        let rx: u64 = fields[0].parse().unwrap_or(0);
        let tx: u64 = fields[8].parse().unwrap_or(0);
        total.rx_bytes = total.rx_bytes.saturating_add(rx);
        total.tx_bytes = total.tx_bytes.saturating_add(tx);
    }
    total
}

/// Bytes/sec for rx and tx given two samples and the elapsed wall time.
/// Returns `(0.0, 0.0)` if `elapsed_secs <= 0` (first sample, or a clock
/// hiccup) rather than dividing by zero.
pub fn net_rate(prev: &NetCounters, cur: &NetCounters, elapsed_secs: f64) -> (f64, f64) {
    if elapsed_secs <= 0.0 {
        return (0.0, 0.0);
    }
    let rx = cur.rx_bytes.saturating_sub(prev.rx_bytes) as f64 / elapsed_secs;
    let tx = cur.tx_bytes.saturating_sub(prev.tx_bytes) as f64 / elapsed_secs;
    (rx, tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET_DEV: &str = "Inter-|   Receive                                                |  Transmit\n \
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n \
    lo: 1000       10    0    0    0     0          0         0     1000       10    0    0    0     0       0          0\n \
  eth0: 500000    100    0    0    0     0          0         0    250000       50    0    0    0     0       0          0\n \
 veth1:  20000     20    0    0    0     0          0         0     10000       10    0    0    0     0       0          0\n";

    #[test]
    fn sums_non_loopback_interfaces() {
        let c = parse_proc_net_dev(NET_DEV);
        assert_eq!(c.rx_bytes, 500000 + 20000);
        assert_eq!(c.tx_bytes, 250000 + 10000);
    }

    #[test]
    fn excludes_loopback() {
        let c = parse_proc_net_dev("    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n");
        assert_eq!(c, NetCounters::default());
    }

    #[test]
    fn rate_divides_delta_by_elapsed_seconds() {
        let a = NetCounters {
            rx_bytes: 1000,
            tx_bytes: 2000,
        };
        let b = NetCounters {
            rx_bytes: 3000,
            tx_bytes: 2500,
        };
        let (rx, tx) = net_rate(&a, &b, 2.0);
        assert_eq!(rx, 1000.0);
        assert_eq!(tx, 250.0);
    }

    #[test]
    fn zero_elapsed_is_zero_not_nan() {
        let a = NetCounters::default();
        let (rx, tx) = net_rate(&a, &a, 0.0);
        assert_eq!((rx, tx), (0.0, 0.0));
    }
}

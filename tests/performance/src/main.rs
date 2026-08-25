//! Performance benchmark for valkey-roaring, replicating redis-roaring's
//! tests/performance.c: same dataset (CRoaring census1881), same operation
//! groups, same key layout, and the same output table format so results can
//! be spliced into README.md between BEGIN_PERFORMANCE/END_PERFORMANCE.
//!
//! Each row measures full client round-trip latency per command against a
//! live server, using Welford's online algorithm for mean and st.dev.
//!
//! Usage: performance [census1881-dir]
//!   VALKEY_HOST / VALKEY_PORT  server address (default 127.0.0.1:6379)
//!   PERF_MAX_FILES             limit dataset files (smoke tests)

use std::env;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Instant;

// ============================================================
// Minimal RESP2 client
// ============================================================

struct Client {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl Client {
    fn connect(host: &str, port: u16) -> std::io::Result<Self> {
        let stream = TcpStream::connect((host, port))?;
        stream.set_nodelay(true)?;
        let reader = BufReader::new(stream.try_clone()?);
        let writer = BufWriter::new(stream);
        Ok(Client { reader, writer })
    }

    /// Send one command and read one reply. Error replies are consumed and
    /// tolerated (the vanilla server lacks some measured commands, matching
    /// the upstream harness which frees error replies without checking).
    fn command(&mut self, args: &[&str]) -> std::io::Result<bool> {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
        for a in args {
            buf.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
            buf.extend_from_slice(a.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        self.writer.write_all(&buf)?;
        self.writer.flush()?;
        self.read_reply()
    }

    /// Returns Ok(false) for error replies, Ok(true) otherwise.
    fn read_reply(&mut self) -> std::io::Result<bool> {
        let mut kind = [0u8; 1];
        self.reader.read_exact(&mut kind)?;
        let line = self.read_line()?;
        match kind[0] {
            b'+' | b':' => Ok(true),
            b'-' => Ok(false),
            b'$' => {
                let n: i64 = line.parse().unwrap_or(-1);
                if n >= 0 {
                    let mut data = vec![0u8; n as usize + 2];
                    self.reader.read_exact(&mut data)?;
                }
                Ok(true)
            }
            b'*' => {
                let n: i64 = line.parse().unwrap_or(-1);
                for _ in 0..n.max(0) {
                    self.read_reply()?;
                }
                Ok(true)
            }
            other => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unexpected RESP type byte: {}", other as char),
            )),
        }
    }

    fn read_line(&mut self) -> std::io::Result<String> {
        let mut line = Vec::with_capacity(16);
        let mut byte = [0u8; 1];
        loop {
            self.reader.read_exact(&mut byte)?;
            if byte[0] == b'\r' {
                self.reader.read_exact(&mut byte)?; // consume \n
                break;
            }
            line.push(byte[0]);
        }
        Ok(String::from_utf8_lossy(&line).into_owned())
    }
}

// ============================================================
// Statistics — Welford's online algorithm, printed like upstream
// ============================================================

struct Statistics {
    operation: String,
    n: u64,
    mean_ns: f64,
    m2: f64,
    errors: u64,
}

impl Statistics {
    fn new(operation: &str) -> Self {
        Statistics {
            operation: operation.to_string(),
            n: 0,
            mean_ns: 0.0,
            m2: 0.0,
            errors: 0,
        }
    }

    fn record(&mut self, ns: f64, ok: bool) {
        self.n += 1;
        if !ok {
            self.errors += 1;
        }
        let delta = ns - self.mean_ns;
        self.mean_ns += delta / self.n as f64;
        let delta2 = ns - self.mean_ns;
        self.m2 += delta * delta2;
    }

    fn print(&self) {
        let mean_us = 1e-3 * self.mean_ns;
        let stdev_us = if self.n > 0 {
            1e-3 * (self.m2 / self.n as f64).sqrt()
        } else {
            0.0
        };
        println!("| {:>16} | {:>16.2} | {:>16.2} |", self.operation, mean_us, stdev_us);
        if self.errors > 0 {
            // Outside the '^|'-grep pattern so it never lands in the README table.
            eprintln!(
                "note: {}/{} replies to '{}' were errors (command unsupported by this server)",
                self.errors, self.n, self.operation
            );
        }
    }
}

fn timed(client: &mut Client, stats: &mut Statistics, args: &[&str]) {
    let start = Instant::now();
    let ok = client.command(args).expect("connection lost");
    let ns = start.elapsed().as_nanos() as f64;
    stats.record(ns, ok);
}

// ============================================================
// Dataset loading — comma/whitespace-separated integers per .txt file
// ============================================================

fn read_dataset(dir: &Path, max_files: usize) -> Vec<Vec<u32>> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read dataset dir {}: {}", dir.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "txt"))
        .collect();
    files.sort();
    files.truncate(max_files);
    assert!(!files.is_empty(), "no .txt files in {}", dir.display());

    files
        .iter()
        .map(|p| {
            fs::read_to_string(p)
                .unwrap()
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap())
                .collect()
        })
        .collect()
}

// ============================================================
// Benchmark groups — mirrors tests/performance.c exactly
// ============================================================

fn main() {
    let dataset_dir = env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/performance/data/census1881".to_string());
    let host = env::var("VALKEY_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = env::var("VALKEY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6379);
    let max_files: usize = env::var("PERF_MAX_FILES")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(usize::MAX);

    let numbers = read_dataset(Path::new(&dataset_dir), max_files);
    let count = numbers.len();
    let total: usize = numbers.iter().map(|v| v.len()).sum();
    eprintln!("dataset: {} files, {} values from {}", count, total, dataset_dir);

    let mut c = Client::connect(&host, port).expect("cannot connect to server");

    println!("| {:>16} | {:>16} | {:>16} |", "OP", "TIME/OP (us)", "ST.DEV. (us)");
    println!("| {0} | {0} | {0} |", "----------------");

    // SETBIT: bits 1, 0, 1 per value — key "{op}-{i}"
    {
        let ops = ["R.SETBIT", "R64.SETBIT", "SETBIT"];
        for (op, name) in ops.iter().enumerate() {
            let mut stats = Statistics::new(name);
            for bit in ["1", "0", "1"] {
                for (i, vals) in numbers.iter().enumerate() {
                    let key = format!("{}-{}", op, i);
                    for v in vals {
                        timed(&mut c, &mut stats, &[name, &key, &v.to_string(), bit]);
                    }
                }
            }
            stats.print();
        }
    }

    // GETBIT
    {
        let ops = ["R.GETBIT", "R64.GETBIT", "GETBIT"];
        for (op, name) in ops.iter().enumerate() {
            let mut stats = Statistics::new(name);
            for (i, vals) in numbers.iter().enumerate() {
                let key = format!("{}-{}", op, i);
                for v in vals {
                    timed(&mut c, &mut stats, &[name, &key, &v.to_string()]);
                }
            }
            stats.print();
        }
    }

    // BITCOUNT
    {
        let ops = ["R.BITCOUNT", "R64.BITCOUNT", "BITCOUNT"];
        for (op, name) in ops.iter().enumerate() {
            let mut stats = Statistics::new(name);
            for i in 0..count {
                let key = format!("{}-{}", op, i);
                timed(&mut c, &mut stats, &[name, &key]);
            }
            stats.print();
        }
    }

    // BITPOS: bits 1, 0
    {
        let ops = ["R.BITPOS", "R64.BITPOS", "BITPOS"];
        for (op, name) in ops.iter().enumerate() {
            let mut stats = Statistics::new(name);
            for bit in ["1", "0"] {
                for i in 0..count {
                    let key = format!("{}-{}", op, i);
                    timed(&mut c, &mut stats, &[name, &key, bit]);
                }
            }
            stats.print();
        }
    }

    // BITOP NOT: dest "dest-{op}-{i}", src "{op}-{i}"
    {
        let ops = ["R.BITOP", "R64.BITOP", "BITOP"];
        for (op, name) in ops.iter().enumerate() {
            let mut stats = Statistics::new(&format!("{} NOT", name));
            for i in 0..count {
                let dest = format!("dest-{}-{}", op, i);
                let src = format!("{}-{}", op, i);
                timed(&mut c, &mut stats, &[name, "NOT", &dest, &src]);
            }
            stats.print();
        }
    }

    // BITOP AND/OR/XOR/ANDOR/ONE: dest "dest-{t}-{op}-{i}", srcs "{op}-{2i}" "{op}-{2i+1}"
    {
        let ops = ["R.BITOP", "R64.BITOP", "BITOP"];
        let types = ["AND", "OR", "XOR", "ANDOR", "ONE"];
        for (t, ty) in types.iter().enumerate() {
            for (op, name) in ops.iter().enumerate() {
                let mut stats = Statistics::new(&format!("{} {}", name, ty));
                for i in 0..count {
                    let dest = format!("dest-{}-{}-{}", t, op, i);
                    let s1 = format!("{}-{}", op, 2 * i);
                    let s2 = format!("{}-{}", op, 2 * i + 1);
                    timed(&mut c, &mut stats, &[name, ty, &dest, &s1, &s2]);
                }
                stats.print();
            }
        }
    }

    // MIN / MAX (vanilla MIN/MAX do not exist — error replies, as upstream)
    for group in [["R.MIN", "R64.MIN", "MIN"], ["R.MAX", "R64.MAX", "MAX"]] {
        for (op, name) in group.iter().enumerate() {
            let mut stats = Statistics::new(name);
            for i in 0..count {
                let key = format!("{}-{}", op, i);
                timed(&mut c, &mut stats, &[name, &key]);
            }
            stats.print();
        }
    }
}

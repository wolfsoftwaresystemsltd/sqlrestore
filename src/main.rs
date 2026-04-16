use clap::Parser;
use flate2::read::MultiGzDecoder;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

const BANNER: &str = r#"
   /\___/\
  /       \      sqlrestore v{VERSION}
 |  o   o  |     By Wolf Software Systems Ltd
  \   ^   /      https://wolf.uk.com
   \_____/
"#;

#[derive(Parser, Debug)]
#[command(
    name = "sqlrestore",
    version,
    about = "Fast MariaDB/MySQL dump restore with table exclusion",
    long_about = "Streams a mysqldump .sql (or .sql.gz) file into the mariadb client, \
                  skipping structure and data for any tables named via --exclude."
)]
struct Args {
    /// Suppress the startup banner
    #[arg(long, global = true)]
    quiet: bool,
    /// Database user
    user: String,
    /// Database password ("" for none)
    password: String,
    /// Target database name
    database: String,
    /// Path to the dump file (.sql or .sql.gz)
    file: String,
    /// Comma-separated list of tables to exclude (case-insensitive)
    #[arg(long, value_delimiter = ',')]
    exclude: Vec<String>,
    /// Database host
    #[arg(long, short = 'H', default_value = "localhost")]
    host: String,
    /// Database port
    #[arg(long, short = 'P', default_value_t = 3306)]
    port: u16,
    /// Path to the client binary (mariadb or mysql)
    #[arg(long, default_value = "mariadb")]
    client: String,
    /// Extra args to pass to the client (quoted, comma-separated)
    #[arg(long, value_delimiter = ',')]
    client_arg: Vec<String>,
    /// Disable SSL (passes --ssl=0 to the client)
    #[arg(long)]
    no_ssl: bool,
    /// Don't pipe to a client — write filtered SQL to stdout
    #[arg(long)]
    dry_run: bool,
    /// Don't wrap in speed-tuning SET statements
    #[arg(long)]
    no_tune: bool,
    /// Print progress every N MiB of input (0 = off)
    #[arg(long, default_value_t = 256)]
    progress_mib: u64,
}

const BUF: usize = 1 << 20;

fn main() {
    if let Err(e) = run() {
        eprintln!("sqlrestore: {e}");
        std::process::exit(1);
    }
}

fn run() -> std::io::Result<()> {
    let args = Args::parse();
    if !args.quiet {
        eprintln!("{}", BANNER.replace("{VERSION}", env!("CARGO_PKG_VERSION")));
    }
    let excluded: HashSet<String> = args.exclude.iter().map(|s| s.to_lowercase()).collect();

    let file = File::open(&args.file)
        .map_err(|e| std::io::Error::new(e.kind(), format!("open {}: {e}", args.file)))?;
    let raw: Box<dyn Read> = if args.file.ends_with(".gz") {
        Box::new(MultiGzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut reader = BufReader::with_capacity(BUF, raw);

    let mut child = None;
    let mut writer: Box<dyn Write> = if args.dry_run {
        Box::new(BufWriter::with_capacity(BUF, std::io::stdout().lock()))
    } else {
        let mut cmd = Command::new(&args.client);
        cmd.arg(format!("-u{}", args.user))
            .arg(format!("-h{}", args.host))
            .arg(format!("-P{}", args.port));
        if args.no_ssl {
            cmd.arg("--ssl=0");
        }
        cmd.args(&args.client_arg)
            .arg(&args.database)
            .stdin(Stdio::piped());
        if !args.password.is_empty() {
            cmd.env("MYSQL_PWD", &args.password);
        }
        let mut c = cmd
            .spawn()
            .map_err(|e| std::io::Error::new(e.kind(), format!("spawn {}: {e}", args.client)))?;
        let stdin = c.stdin.take().expect("stdin piped");
        child = Some(c);
        Box::new(BufWriter::with_capacity(BUF, stdin))
    };

    let mut seen_tables: HashSet<String> = HashSet::new();
    let mut skipped_tables: HashSet<String> = HashSet::new();
    let started = Instant::now();
    let mut bytes_in: u64 = 0;
    let mut bytes_out: u64 = 0;

    let result: std::io::Result<()> = (|| {
        if !args.no_tune {
            writer.write_all(
                b"/* sqlrestore: speed tuning */\n\
                  SET @OLD_AUTOCOMMIT=@@AUTOCOMMIT, AUTOCOMMIT=0;\n\
                  SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0;\n\
                  SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0;\n\
                  SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0;\n",
            )?;
        }

        let mut skipping = false;
        let mut line: Vec<u8> = Vec::with_capacity(64 << 10);
        let progress_step = args.progress_mib.saturating_mul(1 << 20);
        let mut next_progress = progress_step;

        loop {
            line.clear();
            let n = reader.read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            bytes_in += n as u64;

            if line.starts_with(b"-- ") {
                match parse_marker(&line) {
                    Marker::Table(name) => {
                        seen_tables.insert(name.clone());
                        let lower = name.to_ascii_lowercase();
                        if excluded.contains(&lower) {
                            skipping = true;
                            skipped_tables.insert(name);
                        } else {
                            skipping = false;
                        }
                    }
                    Marker::Other => {
                        skipping = false;
                    }
                    Marker::Border => {}
                }
            }

            if !skipping {
                writer.write_all(&line)?;
                bytes_out += n as u64;
            }

            if progress_step > 0 && bytes_in >= next_progress {
                let secs = started.elapsed().as_secs_f64().max(0.001);
                eprintln!(
                    "  {} MiB read, {:.1} MiB/s",
                    bytes_in / (1 << 20),
                    (bytes_in as f64 / 1_048_576.0) / secs
                );
                next_progress = next_progress.saturating_add(progress_step);
            }
        }

        if !args.no_tune {
            writer.write_all(
                b"COMMIT;\n\
                  SET AUTOCOMMIT=@OLD_AUTOCOMMIT;\n\
                  SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS;\n\
                  SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS;\n\
                  SET SQL_NOTES=@OLD_SQL_NOTES;\n",
            )?;
        }
        writer.flush()
    })();
    drop(writer);

    if let Some(mut c) = child {
        let status = c.wait()?;
        match (&result, status.success()) {
            (Err(e), false) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                return Err(std::io::Error::other(format!(
                    "client exited early with {status}"
                )));
            }
            _ => {}
        }
        result?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "client exited with {status}"
            )));
        }
    } else {
        result?;
    }

    let elapsed = started.elapsed();
    let secs = elapsed.as_secs_f64().max(0.001);
    eprintln!(
        "done: read {:.1} MiB, wrote {:.1} MiB in {:.2}s ({:.1} MiB/s in, {:.1} MiB/s out), {} tables seen, {} excluded",
        bytes_in as f64 / 1_048_576.0,
        bytes_out as f64 / 1_048_576.0,
        secs,
        (bytes_in as f64 / 1_048_576.0) / secs,
        (bytes_out as f64 / 1_048_576.0) / secs,
        seen_tables.len(),
        skipped_tables.len(),
    );

    let missing: Vec<&String> = excluded
        .iter()
        .filter(|e| !seen_tables.iter().any(|s| s.to_ascii_lowercase() == **e))
        .collect();
    if !missing.is_empty() {
        let mut m: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        m.sort();
        eprintln!(
            "warning: --exclude listed tables not found in dump: {}",
            m.join(", ")
        );
    }

    Ok(())
}

enum Marker {
    Table(String),
    Other,
    Border,
}

fn parse_marker(line: &[u8]) -> Marker {
    let prefixes: [&[u8]; 4] = [
        b"-- Table structure for table `",
        b"-- Dumping data for table `",
        b"-- Temporary table structure for view `",
        b"-- Final view structure for view `",
    ];
    for p in prefixes {
        if line.starts_with(p) {
            let rest = &line[p.len()..];
            if let Some(end) = rest.iter().position(|&c| c == b'`') {
                return Marker::Table(String::from_utf8_lossy(&rest[..end]).into_owned());
            }
        }
    }
    if line == b"--\n" || line == b"--\r\n" {
        return Marker::Border;
    }
    Marker::Other
}

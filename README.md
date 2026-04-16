# sqlrestore

[![CI](https://github.com/wolfsoftwaresystemsltd/sqlrestore/actions/workflows/ci.yml/badge.svg)](https://github.com/wolfsoftwaresystemsltd/sqlrestore/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/wolfsoftwaresystemsltd/sqlrestore?logo=github)](https://github.com/wolfsoftwaresystemsltd/sqlrestore/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Sponsor](https://img.shields.io/badge/Sponsor-Wolf%20Software%20Systems-ea4aaa?logo=githubsponsors&logoColor=white)](https://github.com/sponsors/wolfsoftwaresystemsltd)

Fast MariaDB / MySQL dump restore with **per-table exclusion**.

`sqlrestore` streams a `mysqldump`-format `.sql` (or `.sql.gz`) file straight
into the `mariadb` client, skipping the structure and data of any tables you
name on the command line. It never loads the dump into memory, so it works on
dumps of arbitrary size.

Typical use case: you have a 200 GB nightly dump that includes a giant
`assets` / `assetcache` table you don't actually want on this machine. Restore
everything else in one pass without unpacking, editing, or pre-processing the
dump.

---

## Sponsor

If `sqlrestore` saves you time, please consider sponsoring our work — it
funds the open-source tools we publish.

<p>
  <a href="https://github.com/sponsors/wolfsoftwaresystemsltd">
    <img src="https://img.shields.io/badge/%E2%9D%A4%20Sponsor-Wolf%20Software%20Systems-ea4aaa?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor on GitHub"/>
  </a>
</p>

## Features

- Streams the dump line-by-line — handles dumps far larger than RAM.
- Filters whole table sections (DROP, CREATE, LOCK, INSERT, UNLOCK) by parsing
  the `mysqldump` section markers — no SQL re-parsing required.
- Transparent `.gz` decompression (multi-member safe).
- Pipes filtered SQL into the `mariadb` (or `mysql`) client over a 1 MiB
  buffered pipe.
- Wraps the session in `AUTOCOMMIT=0`, `UNIQUE_CHECKS=0`,
  `FOREIGN_KEY_CHECKS=0`, `SQL_NOTES=0` and a single trailing `COMMIT` for
  significantly faster InnoDB restores.
- Password is passed via the `MYSQL_PWD` environment variable, never on the
  command line.
- `--dry-run` mode prints the filtered SQL to stdout for inspection.
- Single static binary — no runtime dependencies, runs on any Linux distro.

## Install

### One-liner (recommended)

Statically-linked binaries for `x86_64` and `aarch64` are published with every
release, so you don't need a Rust toolchain to install:

```sh
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/sqlrestore/main/setup.sh | bash
```

Pin a version or change the install prefix:

```sh
curl -fsSL https://raw.githubusercontent.com/wolfsoftwaresystemsltd/sqlrestore/main/setup.sh \
    | VERSION=v0.1.0 PREFIX=$HOME/.local bash
```

### Manual download

Grab the right tarball for your CPU from the
[releases page](https://github.com/wolfsoftwaresystemsltd/sqlrestore/releases/latest):

| Platform                   | Asset                                    |
| -------------------------- | ---------------------------------------- |
| Linux x86_64 (Intel/AMD)   | `sqlrestore-<ver>-linux-x86_64.tar.gz`   |
| Linux aarch64 (ARM64)      | `sqlrestore-<ver>-linux-aarch64.tar.gz`  |

```sh
tar -xzf sqlrestore-*-linux-x86_64.tar.gz
sudo install -m 0755 sqlrestore /usr/local/bin/
```

Each tarball ships with a matching `.sha256` sidecar.

### Build from source

Requires Rust 1.74+ and the `mariadb` (or `mysql`) client binary on `PATH`.

```sh
git clone https://github.com/wolfsoftwaresystemsltd/sqlrestore.git
cd sqlrestore
cargo build --release
# binary at ./target/release/sqlrestore
```

## Usage

```
sqlrestore <user> <password> <database> <file> [options]
```

| Argument          | Description                                          |
| ----------------- | ---------------------------------------------------- |
| `<user>`          | Database user                                        |
| `<password>`      | Database password (use `""` for none)                |
| `<database>`      | Target database name (must already exist)            |
| `<file>`          | Path to dump file (`.sql` or `.sql.gz`)              |

| Option                  | Default     | Description                                   |
| ----------------------- | ----------- | --------------------------------------------- |
| `--exclude a,b,c`       | (none)      | Comma-separated list of tables to skip        |
| `-H, --host`            | `localhost` | Database host                                 |
| `-P, --port`            | `3306`      | Database port                                 |
| `--client`              | `mariadb`   | Client binary (e.g. `mysql`)                  |
| `--client-arg a,b`      | (none)      | Extra args passed through to the client       |
| `--dry-run`             | off         | Print filtered SQL to stdout instead          |
| `--no-tune`             | off         | Skip the speed-tuning `SET` wrapper           |
| `--progress-mib N`      | `256`       | Print progress every N MiB read (`0` to off)  |

## Examples

Restore a dump into `mydb`, skipping two large tables:

```sh
sqlrestore root '' mydb backup.sql --exclude assets,assetcache
```

Restore from a gzipped dump on a remote MariaDB:

```sh
sqlrestore admin 'p@ss' prod_clone backup.sql.gz \
    --exclude audit_log,sessions \
    -H db.internal -P 3306
```

Inspect what would be restored without touching the database:

```sh
sqlrestore u p mydb backup.sql --exclude assets --dry-run | less
```

Use the `mysql` client instead of `mariadb`, with extra client flags:

```sh
sqlrestore root '' mydb backup.sql \
    --client mysql \
    --client-arg --ssl-mode=DISABLED,--local-infile=1
```

## How the table filter works

`mysqldump` always emits section markers like:

```
--
-- Table structure for table `users`
--
...
--
-- Dumping data for table `users`
--
...
```

`sqlrestore` watches for these markers (also the `Temporary table structure
for view` and `Final view structure for view` variants). When a marker names
an excluded table, every following line is dropped until the next section
marker. That cleanly removes the table's `DROP`, `CREATE`, `LOCK TABLES`,
`INSERT`, `UNLOCK TABLES`, and trigger statements as a single unit, without
having to tokenize SQL.

Table name matching is case-insensitive.

## Caveats

- The target database must already exist; `sqlrestore` does not run `CREATE
  DATABASE`.
- Foreign key constraints from kept tables that reference excluded tables
  will fail at `CREATE TABLE` time. Either also exclude the dependent table or
  drop the foreign key from the source schema.
- `sqlrestore` only understands dumps produced by `mysqldump`/`mariadb-dump`.
  Hand-crafted SQL files without the standard section markers are passed
  through unchanged.

## Releases & CI

Pushes to `main` and pull requests are validated by the
[CI workflow](.github/workflows/ci.yml) (rustfmt, clippy, build, test on both
glibc and musl).

Tagging `vX.Y.Z` triggers the
[release workflow](.github/workflows/release.yml), which cross-builds static
binaries for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` and
attaches them — with SHA-256 sidecars — to the GitHub release.

## Contributors

- [Wolf Software Systems Ltd](https://github.com/wolfsoftwaresystemsltd) — maintainer
- Fang the AI — implementation & docs

Contributions welcome — please open an issue or PR.

## License

MIT. See [LICENSE](LICENSE).

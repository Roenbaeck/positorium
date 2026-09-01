# Getting started with the Positorium beta

This guide takes a beta tester from a downloaded archive or source checkout to
a working local query, a restart check, and a verified backup. Positorium is an
experimental single-process database. The beta HTTP service is intended for a
trusted local machine and has no authentication; do not expose it to an
untrusted network.

## Choose how to try it

### Browser-only testbed

Open the [Positorium Query Studio](https://roenbaeck.github.io/positorium/) and
run the preloaded **Facts can disagree** example. Local WASM is enabled
automatically for a first-time visitor, so the example runs entirely in the
browser, writes no native store, and needs no installation. The editor is saved
locally, while refreshing the page discards the in-memory database.

### Native release archive

Download the archive and matching `.sha256` file for your platform from
[GitHub Releases](https://github.com/Roenbaeck/positorium/releases):

| Platform | Archive label |
| --- | --- |
| Linux x86-64 | `linux-x86_64` |
| macOS Apple Silicon | `macos-aarch64` |
| Windows x86-64 | `windows-x86_64` |

Verify the checksum before extracting it.

macOS:

```sh
shasum -a 256 -c positorium-vX.Y.Z-beta.N-macos-aarch64.tar.gz.sha256
```

Linux:

```sh
sha256sum -c positorium-vX.Y.Z-beta.N-linux-x86_64.tar.gz.sha256
```

Windows PowerShell:

```powershell
$archive = "positorium-vX.Y.Z-beta.N-windows-x86_64.zip"
$expected = (Get-Content "$archive.sha256").Split()[0]
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "Checksum mismatch" }
```

Extract the archive and keep its layout intact. In particular,
`positorium.json` and `traqula/empty.traqula` must remain beside and below the
native binary as packaged.

### Build from source

Install [rustup](https://rustup.rs/), clone the repository, and run:

```sh
cargo build --release --locked
```

The server and maintenance binaries are `target/release/positorium` and
`target/release/positorium-store`. Run them from the repository root so the
default configuration and startup script resolve correctly.

## Start the native server

From the extracted archive or repository root:

macOS or Linux:

```sh
./positorium
```

When building from source, use `./target/release/positorium` instead.

Windows PowerShell:

```powershell
.\positorium.exe
```

When building from source, use `.\target\release\positorium.exe` instead.

The first start creates `positorium.store`, executes the empty startup script,
and listens on `127.0.0.1:8080`. Leave this terminal running. Stop the server
with Ctrl-C so its final flush completes.

## Make the first query

In another terminal, send an identity-resolving script. It creates Ada only
when no matching identity exists, then returns the stable Thing identity.

macOS or Linux:

```sh
curl --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  --data '{"traqula_version":1,"script":"add role name; search [{(?person, name), ...}, \"Ada\", *] return ?person or add posit [{(+person, name)}, \"Ada\", @NOW];","stream":false}'
```

Windows PowerShell:

```powershell
$body = @{
  traqula_version = 1
  script = 'add role name; search [{(?person, name), ...}, "Ada", *] return ?person or add posit [{(+person, name)}, "Ada", @NOW];'
  stream = $false
} | ConvertTo-Json
Invoke-RestMethod http://127.0.0.1:8080/v1/query `
  -Method Post -ContentType application/json -Body $body
```

A successful response has `"status":"ok"`, one `thing` result cell, and
`"limited":false`. Run the same request again: it should return the same Thing
identity rather than creating another matching person.

## Confirm persistence

Stop the server with Ctrl-C, start it again from the same directory, and repeat
the request. The Thing identity should remain unchanged. Inspect the store while
the server is stopped:

macOS or Linux:

```sh
./positorium-store inspect positorium.store
```

Windows PowerShell:

```powershell
.\positorium-store.exe inspect positorium.store
```

The command prints a JSON report and exits nonzero if validation fails.

## Create a backup

Stop the writer before offline maintenance, choose a destination that does not
already exist, and run:

```sh
./positorium-store backup positorium.store backup.store
./positorium-store inspect backup.store
```

On Windows, add the `.exe` suffix. Keep backups outside the live store
directory. For portable logical transfer and identity-remapping rules, follow
the [transfer reference](reference/TRANSFER.md).

## Configuration

The packaged `positorium.json` is deliberately explicit:

```json
{
  "listen_interface": "127.0.0.1",
  "listen_port": 8080,
  "database_file_and_path": "positorium.store",
  "enable_persistence": true,
  "recreate_database_on_startup": false,
  "traqula_file_to_run_on_startup": "traqula/empty.traqula"
}
```

- Keep `listen_interface` on loopback. A non-loopback binding is still
  unauthenticated and is not safe for Internet exposure.
- Set `enable_persistence` to `false` for an ephemeral native session.
- `recreate_database_on_startup` deletes the configured store before opening;
  leave it `false` unless destructive recreation is intentional.
- Relative store and startup paths are resolved from the process working
  directory, so start the binary from the directory containing the config.
- See [Operations](guides/OPERATIONS.md) for CORS, request size, timeout,
  recovery, and durability settings.

## Query Studio with a native server

The hosted Query Studio should use Local WASM. To connect the development UI to
the native server, serve the repository on a loopback origin and explicitly
allow that exact origin in `positorium.json`, for example:

```json
"cors_allowed_origins": "http://127.0.0.1:8000"
```

Then serve the checkout with a local static server, open `positorium.html`,
disable Local WASM in Settings, and set the endpoint to
`http://127.0.0.1:8080/v1/query`. Do not use a wildcard CORS origin.

## Troubleshooting

### The config or startup script cannot be read

Run the binary from the extracted archive directory or repository root. Confirm
that `positorium.json` and `traqula/empty.traqula` both exist without flattening
the archive.

### Port 8080 is already in use

Stop the other listener or change `listen_port` in `positorium.json`, then use
the same port in your query URL.

### The store will not open

Do not delete or edit store files in place. Stop any other Positorium process,
run `positorium-store inspect`, and follow the recovery rules in
[Operations](guides/OPERATIONS.md). Restore a verified backup when committed
corruption is reported.

### More diagnostics are needed

Set `RUST_LOG=debug,positorium=info` before starting the server. Avoid attaching
private data or complete store files to a public report.

## Report beta feedback

Open a [GitHub issue](https://github.com/Roenbaeck/positorium/issues) with:

- the beta version and platform archive label;
- whether you used native, HTTP, SSE, or WASM execution;
- the smallest Traqula script that reproduces the problem;
- the expected and actual result or error; and
- relevant logs with secrets and sensitive literal values removed.

Before relying on behavior as a compatibility promise, consult the independent
versions in [Contracts](reference/CONTRACTS.md). Continue with the
[Traqula reference](reference/TRAQULA.md) and [Cookbook](guides/COOKBOOK.md).

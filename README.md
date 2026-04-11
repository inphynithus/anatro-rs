# anatro-rs

Audio fingerprint scanner for detecting intro and outro boundaries in media files.
Designed for batch processing of episodic content using acoustic fingerprinting via Chromaprint
and cross-correlation refinement via FFT-based convolution.

## Requirements

- [chromaprint](https://acoustid.org/chromaprint) — must be installed and discoverable via the
  linker (e.g. `libchromaprint-dev` on Debian/Ubuntu, `chromaprint` on Arch).
- Rust toolchain (see `rust-toolchain.toml`).

## Build

```
cargo build --release
```

The binary will be at `target/release/anatro-rs`.

## Usage

```
anatro-rs [--log [LEVEL]] <COMMAND>
```

### Commands

**scan** — Scan a directory or single file for intro/outro boundaries.

```
anatro-rs scan --target <DIR> --sample-reference <FILE> --sample-intro <MM:SS>
anatro-rs scan --file <FILE>  --sample-reference <FILE> --sample-outro <MM:SS>
```

**Flags**

| Flag                        | Default | Description                                          |
|-----------------------------|---------|------------------------------------------------------|
| `--target <DIR>`            | —       | Directory containing `.mkv`/`.mp4` files             |
| `-f, --file <FILE>`         | —       | Single media file to process                         |
| `--sample-reference <FILE>` | —       | Reference episode (filename or absolute path)        |
| `--sample-intro <MM:SS>`    | —       | Timestamp of the intro in the reference episode      |
| `--sample-outro <MM:SS>`    | —       | Timestamp of the outro in the reference episode      |
| `--sample-size <SECONDS>`   | `10.0`  | Duration of the extracted reference sample           |
| `--preset <NAME>`           | first   | Named heuristic preset from `presets.json`           |
| `--offset <SECONDS>`        | `0.0`   | Apply a fixed offset to all reported timestamps      |
| `-t, --threads <N>`         | `4`     | Worker threads for parallel scanning                 |
| `-p, --progress`            | off     | Show per-thread progress spinners                    |
| `--force`                   | off     | Bypass the cache and re-scan every file              |
| `--json`                    | off     | Write results as JSON to stdout                      |

**debug** — Detailed single-file diagnostic with correlation peak analysis.

```
anatro-rs debug -f <FILE> -e <SECONDS> --sample-reference <FILE> --sample-intro <MM:SS>
```

**sample-extract** — Export a PCM WAV clip for offline inspection.

```
anatro-rs sample-extract -t <FILE> -r HH:MM:SS-HH:MM:SS -o <OUTPUT>
```

## Supported Formats

Decoding is handled by [Symphonia](https://github.com/pdeljanov/Symphonia). Any container and
codec combination supported by Symphonia is accepted. Common tested configurations:

| Container | Codecs                        |
|-----------|-------------------------------|
| MKV       | AAC, FLAC, MP3, Opus, Vorbis  |
| MP4       | AAC, FLAC                     |

## Presets

On first run, a default preset file is created at:

```
$XDG_CONFIG_HOME/anatro-rs/presets.json   (Linux)
~/Library/Application Support/anatro-rs/presets.json   (macOS)
```

A preset defines the normalised search bounds (fraction of total duration) and expected segment
duration used during coarse matching. The first entry in the file is used when `--preset` is
not specified.

```json
{
  "anime_default": {
    "intro": {
      "search_bounds": [0.0, 0.25],
      "intro_duration": 90.0,
      "offset": 0.0
    },
    "outro": {
      "search_bounds": [0.75, 1.0],
      "outro_duration": 90.0,
      "offset": 0.0
    }
  }
}
```

## Caching

Scan results are persisted in a key-value file store:

```
$XDG_CONFIG_HOME/anatro-rs/cache/
```

Each file is keyed by FNV-1a 64-bit hash of its filename. Cached entries are skipped
automatically on subsequent runs. Use `--force` to invalidate the cache for all targets.
Pass `--json` to disable cache writes entirely and print results to stdout instead.

## External Reference Samples

`--sample-reference` accepts an absolute path to a file that is not part of the scanned set.
This is useful when the canonical reference episode is not in the batch directory. An offset
may be required to compensate for encoding differences between the external file and the
targets; use `--offset` or the `offset` field in `presets.json` for this.

## Logging

```
anatro-rs --log [debug|info|warn] scan ...
```

The `RUST_LOG` environment variable takes precedence and accepts the standard `env_logger`
filter syntax for fine-grained control (e.g. `RUST_LOG=anatro_rs=debug,symphonia=warn`).

## License

See repository root for licensing information.

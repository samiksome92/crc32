# crc32

Simple command-line utility to compute the CRC32 checksums of files. Can also create and verify SFV files.

## Installation
Either download a release directly from [releases](https://github.com/samiksome92/crc32/releases) or use `cargo`:

    cargo install --git https://github.com/samiksome92/crc32

## Usage
    crc32 [OPTIONS] <PATHS>...

Arguments:

    <PATHS>...  File and directory paths.

Options:

    -r, --recursive            Parse directories recursively.
    -o, --out-file <OUT_FILE>  Output file name.
    -v, --verify               Verify a checksum file.
    -h, --help                 Print help
    -V, --version              Print version

Scans all given paths for files and computes their CRC32 checksums. If `--recursive` is specified, directories are searched recursively for files. If `--out-file` is provided an output file in SFV format is written.

If `--verify` is specified, only the first path is used. It is assumed to be a SFV file, which is then verified.
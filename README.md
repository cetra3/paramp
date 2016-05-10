# Parashift AMP Extractor

This tool will read a given yaml file and extract all amps present to a
specified directory


## Compilation

If you haven't already, download rust:
```
curl -sSf https://static.rust-lang.org/rustup.sh | sh
```

Once downloaded run the following cargo command:

```
cargo build --release
```

## Usage

Usage is via command line:

```
paramp source.yaml output
```

Where `source.yaml` is in the following format:

```
amps:
  - example-module.amp
  - example-module2.amp
```

And `output` is the output directory
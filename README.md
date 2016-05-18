# Parashift AMP Extractor

This tool will read a given yaml file and extract all amps present to a specified directory.  It is meant as a replacement for the more restrictive and cumbersome alfresco module management tool

## Compilation

If you haven't already, download rust:

```
curl -sSf https://static.rust-lang.org/rustup.sh | sh
```

Once downloaded run the following cargo command:

```
cargo build --release
```

This will create a binary in `target/release/paramp`

## Usage

Usage is via command line:

```
paramp source.yaml <output_dir>
```

Where `source.yaml` is a yaml file dictating what amps and war to use and `output_dir` is an optional directory to output the file.  If you omit `output_dir` in the command line, then you should include it within the yaml file.

## Yaml Format

The Yaml file format is in the following format:

```
files:
  - example-module.amp
  - example-module2.amp
  - /path/to/alfresco.war

output_dir: /var/lib/tomcat7/webapps/alfresco
```

`files` is a list of amps and a war file.  They can theoretically be any zip file, but will try and copy `module.properties` to the correct location if present.

`output_dir` is an optional property which specifies where to output the extracted files.

If you don't specify `output_dir` within the yaml file, then you will need to specify it via the command line.  If you specify it via the command line, then it will override what's in the yaml file.

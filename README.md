# Parashift AMP Extractor

This tool will read a given yaml file and extract all amps present to a specified directory.  It is meant as a replacement for the alfresco module management tool (alfresco mmt).


## Advantages over MMT

* It outputs to a directory rather than a war file.  This means that the original war file is unchanged by design, and you can keep it vanilla.

* There is not any restrictions as to what modules are installed in what order.  If your modules have dependencies on other modules, mmt will fail if they aren't installed first.  This tool assumes the user will handle dependencies themselves.

* It's self documenting: you know what modules you've installed as they're in the Yaml file.  A directory of amps may not have all amps installed, or the war may have extra ones installed.

* Easy to upgrade to a new version: just replace the war and run it again.

* It's lightweight and fast and doesn't require java to run.  Takes a few seconds to create a directory.

* Great for bundling a standalone docker image or use via server orchestration such as Salt Stack.

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

## Output directory

The output directory will be cleared to replace the files within it and will create a ready to use web application to deploy to a servlet engine such a tomcat.

### Creating a war

You can still create a war from this directory for packaging.

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

If there are modules that have files in the same location: last write wins. For share.war you should specify this as the last file, as the manifest.mf can be overwritten with another amp

`output_dir` is an optional property which specifies where to output the extracted files.

If you don't specify `output_dir` within the yaml file, then you will need to specify it via the command line.  If you specify it via the command line, then it will override what's in the yaml file.

## Special behaviour

There are some files that are treated specially so that they can be compatible with the existing Module services:

### file-mapping.properties

This file if present in the amp, it will add it to the default mappings

### module.properties

This file if present in the amp, will be copied to the respective directory given by `module.id`:

    /WEB-INF/classes/alfresco/module/<module.id>/module.properties

It will also:

* Strip out any comments or any non-property lines
* Add the following line to the end: `module.installState=INSTALLED`

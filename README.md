[![Build Status](https://travis-ci.org/harababurel/gcsf.svg?branch=master)](https://travis-ci.org/harababurel/gcsf)
[![Crates.io](http://meritbadge.herokuapp.com/gcsf)](https://crates.io/crates/gcsf)
[![Docs](https://docs.rs/gcsf/badge.svg)](https://docs.rs/gcsf/0.1.3/gcsf/)
[![MIT License](http://img.shields.io/badge/license-MIT-blue.svg?style=flat)](https://github.com/harababurel/gcsf/blob/master/LICENSE)

GCSF is a virtual filesystem that allows users to mount their Google Drive account locally and interact with it as a regular disk partition.

### Installation

Make sure you have the `fuse` library installed. GCSF requires the stable branch of the Rust programming language, which can be installed following the instructions on [rustup.rs](https://rustup.rs).

Afterwards, you can simply run:

```bash
$ git clone https://github.com/harababurel/gcsf.git && cd gcsf && cargo build --release
```

This will generate the `./target/release/gcsf` binary.

### Usage

```bash
$ gcsf mount /mnt/gcsf
Please direct your browser to https://accounts.google.com/o/oauth2/[...], follow the instructions and enter the code displayed here:
```

You can now find the contents of your Drive account in `/mnt/gcsf`.

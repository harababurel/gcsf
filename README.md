[![Build Status](https://travis-ci.org/harababurel/gcsf.svg?branch=master)](https://travis-ci.org/harababurel/gcsf)
[![Crates.io](http://meritbadge.herokuapp.com/gcsf)](https://crates.io/crates/gcsf)
[![Docs](https://docs.rs/gcsf/badge.svg)](https://docs.rs/gcsf/latest/gcsf/)
[![MIT License](http://img.shields.io/badge/license-MIT-blue.svg?style=flat)](https://github.com/harababurel/gcsf/blob/master/LICENSE)

GCSF is a virtual filesystem that allows users to mount their Google Drive account locally and interact with it as a regular disk partition. You can find out more in this [paper](https://sergiu.ml/~sergiu/thesis.pdf) (note: it is a draft).
### Installation

Make sure you have the `fuse` library installed. GCSF requires the stable branch of the Rust programming language, which can be installed following the instructions on [rustup.rs](https://rustup.rs).

Afterwards, you can simply run:

```bash
$ cargo install gcsf
```

This will generate the `gcsf` binary in `$HOME/.cargo/bin`. Make sure that this directory is in your `PATH` variable: `export PATH=$PATH:$HOME/.cargo/bin`

### Configuration

GCSF will attempt to create a configuration file in `$XDG_CONFIG_HOME/gcsf/gcsf.toml`, which is usually defined as `$HOME/.config/gcsf/gcsf.toml`.

### Usage

```bash
$ gcsf mount /mnt/gcsf
Please direct your browser to https://accounts.google.com/o/oauth2/[...] and follow the instructions displayed there.
```

You can now find the contents of your Drive account in `/mnt/gcsf`:

![GCSF ls](https://i.imgur.com/jdFIu5Y.png)

Using Ranger:
![GCSF in Ranger](https://i.imgur.com/BuS9BDD.png)

Or Thunar:
![GCSF in Thunar](https://i.imgur.com/9JSDqez.jpg)

### Contributing

Contributions are welcome. You can also help by reporting or fixing [bugs](https://github.com/harababurel/gcsf/issues).

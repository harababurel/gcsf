<img align="right" width="300px" height="300px"
     title="Size Limit logo" src="https://i.imgur.com/9xdFwQq.png">


[![Build Status](https://travis-ci.org/harababurel/gcsf.svg?branch=master)](https://travis-ci.org/harababurel/gcsf)
[![Crates.io](https://img.shields.io/crates/v/gcsf.svg)](https://crates.io/crates/gcsf)
[![Docs](https://docs.rs/gcsf/badge.svg)](https://docs.rs/gcsf/latest/gcsf/)<br>
[![GitHub Issues](https://img.shields.io/github/issues/harababurel/gcsf.svg)](https://github.com/harababurel/gcsf/issues)
[![Crates.io](https://img.shields.io/crates/d/gcsf.svg)](https://crates.io/crates/gcsf)
[![MIT License](https://img.shields.io/crates/l/gcsf.svg)](https://github.com/harababurel/gcsf/blob/master/LICENSE)

GCSF is a virtual filesystem that allows users to mount their Google Drive account locally and interact with it as a regular disk partition. You can find out more in this [paper](https://sergiu.ml/~sergiu/thesis.pdf).

### Requirements

GCSF requires the stable branch of the Rust programming language, which can be installed following the instructions on [rustup.rs](https://rustup.rs). If you already have Rust installed, make sure that it is updated to the latest version (≥1.26):

```bash
$ rustup update stable
```

#### OSX

On Mac OSX, GCSF requires [osxfuse](https://osxfuse.github.io/) and [pkg-config](http://macappstore.org/pkg-config/):

```bash
$ brew update; brew install pkg-config; brew tap homebrew/cask; brew cask install osxfuse
```

#### Ubuntu

On Ubuntu, GCSF requires [libfuse-dev](https://packages.ubuntu.com/trusty/libfuse-dev) and [pkg-config](https://packages.ubuntu.com/xenial/pkg-config):

```bash
sudo apt-get install -y libfuse-dev pkg-config
```

#### Other linux distros

Make sure you have `pkg-config` and the `fuse` library installed. These are usually found in the package repositories of major distributions.

#### Windows

Unfortunately, Windows is not supported at the time being. See issue [#19](https://github.com/harababurel/gcsf/issues/19).

### Installation

After all requirements are met, GCSF can be installed using `cargo`:


```bash
$ cargo install gcsf
```

This will generate the `gcsf` binary in `$HOME/.cargo/bin`. Make sure that this directory is in your `PATH` variable: `export PATH=$PATH:$HOME/.cargo/bin`

Alternatively, you can download a [release binary](https://github.com/harababurel/gcsf/releases) for your platform.

### Configuration

GCSF will attempt to create a configuration file in `$XDG_CONFIG_HOME/gcsf/gcsf.toml`, which is usually defined as `$HOME/.config/gcsf/gcsf.toml`. Credentials are stored in the same directory.

### Usage

The first step is to log in to Drive and authorize the application. A name must be provided for the session:

```bash
$ gcsf login some_session_name
Please direct your browser to https://accounts.google.com/o/oauth2/[...] and follow the instructions displayed there.
Successfully logged in. Saved credentials to "$HOME/.config/gcsf/some_session_name"
```

You can also list all existing sessions:

```bash
$ gcsf list
Sessions:
        - personal
        - some_session_name
        - work
```

And then mount one (or more) of them:

```bash
$ gcsf mount /mnt/gcsf -s some_session_name
INFO  gcsf > Creating and populating file system...
INFO  gcsf > File sytem created.
INFO  gcsf > Mounting to /mnt/gcsf
INFO  gcsf > Mounted to /mnt/gcsf
```

You can now find the contents of your Drive account in `/mnt/gcsf`:

<p align="left">
  <img src="https://i.imgur.com/jdFIu5Y.png" alt="GCSF ls"
       width="530px" height="165px">
</p>

Using Ranger:
<p align="left">
  <img src="https://i.imgur.com/BuS9BDD.png" alt="GCSF in Ranger"
       width="616px" height="351px">
</p>


Or Thunar:
<p align="left">
  <img src="https://i.imgur.com/9JSDqez.jpg" alt="GCSF in Thunar"
       width="746px" height="176px">
</p>

### Contributing

Contributions are welcome. You can also help by reporting or fixing [issues](https://github.com/harababurel/gcsf/issues).

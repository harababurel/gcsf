<img align="right" width="300px" height="300px"
     title="Size Limit logo" src="https://i.imgur.com/9xdFwQq.png">


[![Crates.io](https://img.shields.io/crates/v/gcsf.svg)](https://crates.io/crates/gcsf)
[![Docs](https://docs.rs/gcsf/badge.svg)](https://docs.rs/gcsf/latest/gcsf/)<br>
[![GitHub Issues](https://img.shields.io/github/issues/harababurel/gcsf.svg)](https://github.com/harababurel/gcsf/issues)
[![Downloads](https://img.shields.io/crates/d/gcsf.svg)](https://crates.io/crates/gcsf)
[![MIT License](https://img.shields.io/crates/l/gcsf.svg)](https://github.com/harababurel/gcsf/blob/master/LICENSE)

GCSF is a virtual filesystem that allows users to mount their Google Drive account locally and interact with it as a regular disk partition. You can find out more in this [paper](https://harababurel.com/thesis.pdf)

**Update (April 2019)**: I am currently still using and maintaining this project but I have very little time to dedicate to it. As such, it might take a while before I get around to fixing known bugs / implementing feature requests / responding to open issues. Thank you for understanding and for expressing sustained interest in this project!


### Requirements

GCSF requires the stable branch of the Rust programming language, which can be installed following the instructions on [rustup.rs](https://rustup.rs). If you already have Rust installed, make sure that it is updated to the latest version (≥1.26):

```bash
$ rustup update stable
```

#### OSX

On Mac OSX, GCSF requires [osxfuse](https://osxfuse.github.io/) and [pkg-config](http://macappstore.org/pkg-config/):

```bash
$ brew update; brew install pkg-config; brew tap homebrew/cask; brew install --cask osxfuse
```

#### Ubuntu

On Ubuntu, GCSF requires [libfuse-dev](https://packages.ubuntu.com/disco/libfuse-dev), [libssl-dev](https://packages.ubuntu.com/disco/libssl-dev) and [pkg-config](https://packages.ubuntu.com/disco/pkg-config):

```bash
sudo apt-get install -y libfuse-dev libssl-dev pkg-config
```

#### Fedora

On Fedora, GCSF requires gcc, fuse3-devel, and pkg-config:

```bash
sudo dnf install -y gcc fuse3-devel pkg-config
```

#### Arch Linux

An AUR package is maintained by [axionl](https://github.com/axionl): [gcsf-git](https://aur.archlinux.org/packages/gcsf-git/).

#### SUSE

```bash
sudo zypper install -y fuse-devel fuse rust pkgconf-pkg-config
```

#### Other linux distros

Make sure you have `pkg-config` and the `fuse` library installed. These are usually found in the package repositories of major distributions.

#### FreeBSD

Rust can be installed via the `lang/rust` port. You will need to install `sysutils/fusefs-libs` for the `cairo install` command to succeed.

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

#### GCP

1. Visit [console.developers.google.com](https://console.developers.google.com) and create a new project
2. Add the Google Drive API to the project
3. Configure an OAuth consent screen. Verification should not be required. Should be external unless this project is something internal to your GSuite
4. Configure an OAuth2.0 credential. Do not use WEB as the token type if adding `gcsf` to a headless server - you want to be using the `urn:*` URI (note: if using WEB, you'll need to set the accepted domains to include `http://localhost:8081`)
5. Configure GCSF to use the new `client_id`, `client_secret`, and `project_id`. You should have all these values after creating the credential.
6. Configure GCSF `authorize_using_code=True` if configuring for headless servers. If you do this, completing the OAuth flow in a different browser will provide you a code that you can give to GCSF.

Running `gcsf login some_session_name` at this point should show a URL with your `client_id` query parameter.

#### Publishing to GCP app to Production Mode (Optional, important for long-running services)

If you plan to run GCSF as a system service or for extended periods, it is recommended that your Google Cloud project be **in Production mode**, not Testing mode.

Access tokens for apps in Testing mode expire more frequently, which usually triggers a prompt on GCSF to re-authenticate. This can be inconvenient especially when running GCSF as a system service. Publishing the app to Production mode resolves this issue.

**To publish your app:**
1. Go to [Google Cloud Console](https://console.cloud.google.com)
2. Navigate to "APIs & Services" → "OAuth consent screen" -> "Audience"
3. Check the "Publishing status" at the top
4. If it says "Testing", click **"Publish App"**
5. Publishing to Production might require app verification. Follow the process in the "Verification centre" section.
6. After publishing, you must re-authenticate:
   ```bash
   gcsf logout your_session_name
   gcsf login your_session_name
   ```

**Note:** Publishing to Production does NOT require Google verification for personal use. Verification is only needed if you're distributing your app to many external users.

You can verify your authentication is working at any time:
```bash
$ gcsf verify your_session_name
Verifying authentication for session 'your_session_name'...
Authentication is valid.
```

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
INFO  gcsf > File system created.
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

### Why GCSF?
GCSF stands for "Google Conduce Sistem de Fișiere" which translated from Romanian is "Google Drive Filesystem". However [GDFS](https://github.com/robin-thomas/GDFS) already exists so it remains GCSF.

### Troubleshooting

#### Could not mount to `$mountpoint`: Operation not permitted (os error 1)

This error occurs when `user_allow_other` is not set in `/etc/fuse.conf` or the file has improper permissions. Fix by running (as root):

```bash
# echo 'user_allow_other' >> /etc/fuse.conf
# chmod 644 /etc/fuse.conf
# sudo chown root:root /etc/fuse.conf
```

#### `libssl.so.1.0.0`

You installed the prebuilt binaries but couldn't run it. Fix by installing rust and building from source.

### Contributing

Contributions are welcome. Documentation available on [docs.rs/gcsf](https://docs.rs/gcsf). You can also help by reporting or fixing [issues](https://github.com/harababurel/gcsf/issues).

[![Star History Chart](https://api.star-history.com/svg?repos=harababurel/gcsf&type=Date)](https://star-history.com/#harababurel/gcsf&Date)

<div align="center">

![SysD Manager Icon](data/icons/hicolor/scalable/apps/io.github.plrigaux.sysd-manager.svg "App Icon")

# SysD Manager

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPLv3+-blue.svg)](https://raw.githubusercontent.com/plrigaux/sysd-manager/refs/heads/main/LICENSE)
[![Sponsor](https://img.shields.io/static/v1?label=Sponsor&message=Support&logo=GitHub&color=%23fe8e86&style=flat)](https://github.com/sponsors/plrigaux)
[![Flathub](https://img.shields.io/flathub/v/io.github.plrigaux.sysd-manager?logo=flathub&logoColor=white&label=Flathub)](https://flathub.org/apps/io.github.plrigaux.sysd-manager)

**A user-friendly graphical interface for managing systemd units**

[Features](#-features) • [Installation](#-installation) • [Screenshots](#-screenshots) • [Contributing](#-contributing)

</div>

---

## 📋 Overview

SysD Manager provides an intuitive graphical interface for managing systemd units, making system administration more accessible. The application offers comprehensive functionality for viewing, editing, and controlling systemd services, with features including:

- Enable, disable, start, and stop systemd units
- View and edit unit files with syntax highlighting
- Monitor journal logs in real-time
- Visualize unit dependencies
- Manage unit drop-ins and configurations

**Note:** This application is designed for users who prefer a graphical interface over command-line tools. System administrators may prefer using `systemctl` and `journalctl` directly.

---

## ✨ Features

### Core Functionality

| Feature | Status |
|---------|--------|
| Unit file browser with search and filtering | ✅ |
| Enable/disable units and runtime enablement | ✅ |
| Start, stop, and restart units | ✅ |
| Edit unit files and manage drop-ins | ✅ |
| View and navigate unit dependencies | ✅ |
| Clean, freeze, and thaw operations | ✅ |
| Real-time journal monitoring | ✅ |
| Boot ID retrieval and filtering | ✅ |
| Service creation wizard | ✅ |
| Timer creation wizard | ✅ |
| Mount creation wizard | ✅ |

### User Experience

| Feature | Status |
|---------|--------|
| Syntax highlighting for unit files and journal logs | ✅ |
| Dark and light theme support | ✅ |
| Multi-language support | ✅ |
| Contextual menus and property management | ✅ |
| System signal monitoring | ✅ |
| CLI unit selection at startup | ✅ |
| Favorites | ✅ |
| View for Timers | ✅ |
| View for Path| ✅ |
| View for Sockets| ✅ |
| View for Services | ✅ |
| Views' Unit Files visibility option| ✅ |
---

## 📸 Screenshots

<div align="center">

### Unit Information Panel (Dark Theme)

![Unit Info Dark](screenshots/unit_info_dark.png)

### Unit Information Panel (Light Theme)

![Unit Info Light](screenshots/unit_info.png)

### Dependency Visualization

![Unit Dependencies](screenshots/dependencies_dark.png)

### Unit File Editor

![Unit Files](screenshots/unit_file_dark.png)

### Journal Viewer

![Unit Journal](screenshots/journal_dark.png)

</div>

---

## 🔧 Installation

### Method 1: Flathub (Recommended)

The easiest way to install SysD Manager on any Linux distribution.

<a href="https://flathub.org/apps/io.github.plrigaux.sysd-manager">
  <img width="200" alt="Download on Flathub" src="https://flathub.org/api/badge?svg"/>
</a>

```bash
flatpak install flathub io.github.plrigaux.sysd-manager
```

---

### Method 2: Arch Linux (AUR)

For Arch Linux users, install directly from the Arch User Repository.

<a href="https://aur.archlinux.org/packages/sysd-manager">
  <img width="200" alt="AUR Package" src="https://aur.archlinux.org/static/css/archnavbar/aurlogo.png"/>
</a>

```bash
yay -S sysd-manager
```

Or using any other AUR helper:

```bash
paru -S sysd-manager
```

---

### Method 3: AppImage

Portable application that runs on most Linux distributions.

<a href="https://github.com/plrigaux/sysd-manager/releases/latest">
  <img width="100" alt="Download AppImage" src="https://docs.appimage.org/_images/appimage.svg"/>
</a>

> **Status:** AppImage has been succesfully tested on Debian 12, Ubuntu 24.04 and 26.04

---
### Method 4: RPM

For distros using RPM, ALT Linux is making builds for different archrtectures. 
Packages available [here](https://packages.altlinux.org/en/sisyphus/binary/sysd-manager/).

### Method 5: Nix Package

Before you can download SysD Manager directly from NixO, you can find a build file there.

<a href="https://github.com/plrigaux/sysd-manager/releases/latest">
  <img width="100" alt="Download AppImage" src="data/icons/NixOS.svg"/>
</a>

### Method 6: Deb package

An amd64 Deb package is generated and can be found here on [Latest Release](https://github.com/plrigaux/sysd-manager/releases/latest)

To install run the following
```bash
sudo dpkg -i sysd-mangager-<version>_amd64.deb
```

> **Status:** Deb has been succesfully tested on Ubuntu 26.04

> **Note** There is no PPA yet, because the distro's Rust version is too old :(.

### Method 7: Ubuntu

You can install SysD Manager from PPA for the __stonking__ release

```bash
sudo add-apt-repository ppa:plrigaux/ppa
sudo apt update
sudo apt install sysd-manager
```
        
### Method 8: Build from Source

For developers and users who prefer building from source.

#### System Requirements

Ensure the following dependencies are installed on your system:

| Dependency | Package Name (Debian/Ubuntu) | Package Name (Arch) |
|------------|------------------------------|---------------------|
| Rust toolchain | `cargo rustc` | `rust` |
| GTK 4 development files | `libgtk-4-dev` | `gtk4` |
| Libadwaita development files | `libadwaita-1-dev` | `libadwaita` |
| Systemd development library | `libsystemd-dev` | `systemd-libs` |
| GtkSourceView 5 library | `libgtksourceview-5-dev` | `gtksourceview5` |
| Build essentials | `build-essential` | `base-devel` |

**Additional Resources:**
- [Rust installation guide](https://www.rust-lang.org/tools/install)
- [GTK 4 setup guide](https://gtk-rs.org/gtk4-rs/stable/latest/book/installation_linux.html)
- [Libadwaita setup guide](https://gtk-rs.org/gtk4-rs/stable/latest/book/libadwaita.html)

#### Build Steps

1. Clone the repository:

```bash
git clone https://github.com/plrigaux/sysd-manager
cd sysd-manager/scripts
```

2. Run the installation script:

```bash
sh install.sh
```

3. Launch the application:

```bash
sysd-manager
```

#### Uninstall

To remove the application:

```bash
cd sysd-manager/scripts
sh uninstall.sh
```

---

## Testing

Run all automated checks from the project root:

```bash
scripts/test_i18n.sh
```

This runs Rust catalog validation and its negative tests in debug and release
builds, workspace tests, isolated locale probes, English and Chinese window
tests, formatting, Clippy, Flatpak and
AppImage compile checks, and metadata generation. In addition to the build
dependencies, install gettext tools, `dbus-daemon`,
`glib-compile-schemas`, and `gtk4-broadwayd`. Locale probes use installed UTF-8
locales or generate temporary ones with `localedef` and the glibc locale sources.
The automated checks do not require a Python interpreter.

Window tests use a private Broadway display, D-Bus daemon, in-memory GSettings,
and temporary XDG directories. A mock systemd service verifies start, stop,
restart, reload, job completion/failure, and enable/disable requests without
changing host services. The windows exercise preferences, columns, filters,
status text, service/timer/mount creation, signal monitoring, and file editing
in both languages. Existing tests that require a real systemd installation
remain explicitly ignored; run those separately only in a disposable environment.

GTK warnings are fatal. A generic window/popover/dialog preflight detects runtime
failures before application widgets are constructed. GTK 4.24.1/libadwaita 1.10
can fail this preflight with a frame-timing warning under Broadway; the suite was
also verified with GTK 4.22.5/libadwaita 1.9.4. To use a separate compatible
runtime without changing installed libraries:

```bash
SYSD_GTK_RUNTIME=/path/to/runtime scripts/test_i18n.sh
```

The prefix must contain `usr/lib` and the matching `usr/bin/gtk4-broadwayd`.
For checks that do not need a display, run
`cargo test --workspace --lib --bins` and `cargo test --test i18n`.

For manual testing, the included **tiny_daemon** web server service is available:

```bash
cd packaging
python install_tiny
```

---

## Roadmap

Planned enhancements include:

- Persistent unit filter configurations
- Type-aware property display (e.g., human-readable time formats)
- Administrator mode option
- Ongoing user experience improvements

For the complete list of changes, see the [CHANGELOG](CHANGELOG.md).

---

## 🤝 Contributing

Contributions are welcome. Please contact the project maintainer on [GitHub](https://github.com/plrigaux/sysd-manager) or submit a pull request.

---

## Internationalization

SysD Manager supports multiple languages through community translations.

The interface follows your system language. Simplified Chinese is available as
`zh_Hans`, `zh_CN`, and `zh_SG`. To run in Chinese from the source tree:

```bash
LANGUAGE=zh_CN cargo run
```

Use an installed non-C UTF-8 locale (for example
`LC_ALL=zh_CN.UTF-8 LANGUAGE=zh_CN cargo run`) if your shell uses `C`/`POSIX` or
`C.UTF-8`, which can disable gettext translations. Set
`LANGUAGE=en` to use English. `TEXTDOMAINDIR` can override the catalog directory;
development builds otherwise use the Cargo target directory's `locale` folder
(including custom `--target-dir` paths), and installed builds use
`/usr/share/locale` (`/app/share/locale` for Flatpak, or AppImage's launcher path).

Rust messages use `gettext`/`pgettext`; GTK presentation text uses
`translatable="yes"` and an optional context. Keep systemd directives, D-Bus
identifiers, stored filter codes and user data independent of display labels.
Catalogs are compiled during the build and cached by gettext at runtime.

After adding or changing messages, update and validate the Chinese catalog:

```bash
cargo run -p transtools -- expo --lang zh_Hans
cargo run -p transtools -- check
cargo test --test i18n
```

The check verifies coverage, format placeholders, markup, mnemonics and source
extraction. The runtime test covers locale precedence, Chinese aliases, English
fallback, contextual messages, three-form plurals, catalog path overrides,
daylight-saving transitions and stable filtering identifiers in isolated processes.
With a display available, run
`SYSD_I18N_UI_TEST=1 cargo test --test i18n` to also check GTK Builder translation.
Set `SYSD_I18N_BENCH=1` to print startup, cached lookup and duration formatting
timings. Translations and status label widths are cached after locale setup;
elapsed durations use UTC arithmetic while timestamp displays keep their selected
format and timezone.

<div align="center">
<a href="https://hosted.weblate.org/engage/sysd-manager/" target="_blank">
<img src="https://hosted.weblate.org/widget/sysd-manager/translation/multi-auto.svg" alt="Translation Status" style="height:300px;">
</a>
</div>

Translation services are generously hosted by [Weblate](https://weblate.org). To contribute translations, visit the [SysD Manager Hosted Weblate](https://hosted.weblate.org/engage/sysd-manager/) project page.

For developers working with translations, refer to the [Translation Wiki](https://github.com/plrigaux/sysd-manager/wiki/Translation).

---

## Credits

This project is inspired by the work of Guillaume Gomez: https://github.com/GuillaumeGomez/systemd-manager/

---

## License

SysD Manager is licensed under the GNU General Public License v3.0. See [LICENSE](https://raw.githubusercontent.com/plrigaux/sysd-manager/refs/heads/main/LICENSE) for details.

# Bare-Metal Reality Brief: Shipping i3mlOS as an Installable OS

## 1. "Own OS on a borrowed kernel" — the proven pattern

Every successful example keeps the Linux plumbing and replaces the *session and app model*, never exposing the kernel:

- **Android**: Linux kernel + Bionic libc, Binder IPC, HALs (Project Treble vendor partition), ART runtime, SurfaceFlinger. No GNU userland, no X/Wayland. Users see "Android," period.
- **ChromeOS**: Gentoo-derived base, verified boot (dm-verity), A/B partitions, silent auto-update, Chrome as the shell. Recovery = reflash image.
- **SteamOS 3**: Arch base, immutable read-only root, A/B atomic updates, boots into the Gamescope compositor running Steam — KDE exists but is an escape hatch. Factory reset = Valve's recovery image.
- **Consoles**: PS5's Orbis (FreeBSD-derived) and Switch's Horizon prove the shell *is* the product; the kernel is invisible plumbing.

**Translation for i3mlOS**: keep kernel, systemd, Mesa, PipeWire, NetworkManager, Wayland *protocol*; replace the greeter/session/compositor/app model with the agent shell. AMAN/SIJIL/DHAKIRA sit where Android's system services sit — above the kernel, below the UI.

## 2. Driver and firmware reality, 2026

- **Firmware**: ship the full `linux-firmware` tree (nonfree included) — it covers nearly all Wi-Fi/BT/GPU blobs. Use **fwupd/LVFS** for UEFI/device firmware updates (Framework, Lenovo, Dell all publish there).
- **GPUs**: AMD (amdgpu + RADV) and Intel (i915/xe + ANV) are excellent — make them first-class. NVIDIA's proprietary driver now defaults to **open kernel modules** (driver 560+, mandatory on Blackwell), easing packaging; open-source **NVK** is default GL-via-Zink for Turing+ since Mesa 25.1 but not daily-driver quality, and the Rust **Nova** kernel driver is still bring-up. Tier NVIDIA as "best effort"; Universal Blue's separate `-nvidia` images are the model.
- **Wi-Fi/BT**: Intel AX/BE cards are solid; MediaTek mostly fine; **Realtek and Broadcom remain the pain** (flaky BT coexistence, USB dongles). Reference hardware selection makes this problem disappear.
- **Suspend/battery**: modern laptops dropped S3 for **s2idle**; it works on well-supported machines but firmware bugs cause overnight drain. Budget per-model QA for lid/dock/suspend cycles; ship power-profiles-daemon and amd_pstate/intel_pstate defaults.

## 3. Secure Boot, TPM, encryption

- Getting your own **shim** signed by Microsoft's 3rd-party UEFI CA (via `rhboot/shim-review` on GitHub) takes months, an EV cert, and reproducible builds — and the 2011 signing cert **expired June 2026**; new shims carry the 2023 CA, which older un-updated firmware doesn't trust. This is genuinely messy right now.
- **Pragmatic v1**: base on Fedora/bootc and inherit Fedora's signed shim + GRUB/UKI chain; use **MOK** enrollment for your own kernel modules. `sbctl` (self-enrolled keys) is fine for enthusiasts, wrong for an installer default. "Disable Secure Boot" as documented fallback.
- **Encryption defaults**: LUKS2 everywhere, **TPM2 auto-unlock via systemd-cryptenroll** (optional PIN), systemd-boot + **Unified Kernel Images** with measured boot — this is now the standard modern stack and is a natural fit for SIJIL's tamper-evidence story.

## 4. Installer and updates

- **Calamares** (Manjaro, EndeavourOS, KDE neon) is the sane default: modular, LUKS-capable, brandable — but its partition/resize path needs heavy QA. Build a custom OOBE *on top* only if first-run is the product moment (it is, for an agent OS that needs API keys/consent).
- **Dual-boot with Windows** is the riskiest surface: suspend BitLocker first, shrink NTFS from Windows (safer than ntfsresize), share the ESP, never touch Windows Boot Manager entries. Consider shipping whole-disk "appliance install" first, dual-boot in v1.1.
- **Updates**: this is the highest-leverage decision — ship the OS as an OCI image via **bootc** (Fedora's bootc effort matured through 2025–26; **Universal Blue/Bluefin** prove a tiny team can run image-based A/B updates, automatic rollback via greenboot, from GitHub CI). Factory reset = redeploy base image, wipe `/var`. SteamOS-style recovery USB image doubles as un-brick insurance.

## 5. Reference hardware: certify 2–4 devices

1. **Framework Laptop 13 (AMD)** — Linux-first vendor, officially supports Fedora/Ubuntu, seeds hardware to distros (Bazzite, NixOS, CachyOS), first Ubuntu-certified unit in 2026. Best partner candidate.
2. **ThinkPad T14/X1 Carbon (Intel)** — massive used/corporate base, best-in-class LVFS firmware, well-trodden kernel support.
3. **One AMD mini-PC** (ASUS NUC-class or Beelink) — the "agent server under the desk" story, zero battery/suspend QA burden.
4. (Optional later) one NVIDIA desktop config for local-model users.

How others promise support: **Asahi** publishes brutally honest per-SoC feature matrices (M1/M2 full, M3/M4 tier-3 bring-up) — honesty as policy; **Bluefin/Universal Blue** inherit Fedora's kernel claim ("if Fedora boots, we boot") plus device-specific images (Surface, ASUS, -nvidia); **elementary** partners with OEMs (Star Labs, Slimbook) for preloaded certified units. Copy Asahi: publish a public **Certified / Compatible / Unsupported** matrix and link purchase pages for certified units.

## 6. Try-before-install funnel

Ladder: web video → **VM image** → **live USB** → dual-boot → full install.

- Ship **qcow2 + UTM bundle** (Apple Silicon Macs — your macOS-refugee audience) and VirtualBox OVA/ISO for Windows. Asahi's one-line `curl` installer showed how much friction reduction drives installs of a *new* OS.
- **Live USB with persistence** and an on-desktop "Install" button (Fedora/Ubuntu pattern); make the ISO Ventoy-friendly.
- Caveat: an agent OS's magic (device control, local inference, real hardware) demos poorly in a VM — no GPU, sandboxed peripherals. Position VM = "meet the shell," live USB = "feel it on your hardware," and instrument the funnel (anonymous, opt-in) to measure conversion.

## 7. Minimum app-compat to be daily-drivable

- **Non-negotiable**: a real Chromium-based browser **built in** (agents need CDP for browser control anyway) + Firefox available. This alone covers ~80% of daily computing via web apps.
- **Flathub enabled out of the box**: VS Code, LibreOffice, Zoom, Signal, Spotify, OBS arrive free. Flatpak is the app model — don't invent one for humans.
- **Skip for v1**: **Waydroid** (Android apps — flaky, no sanctioned Play Store, ARM translation hacks) and **Wine/Proton promises** (that's Bazzite/CachyOS territory; offer Steam + Bottles flatpaks, promise nothing).
- **Hidden blockers that kill daily-driving**: printing/scanning (CUPS/IPP-everywhere), webcam+mic in video calls (PipeWire), Bluetooth audio codecs, and emoji/CJK fonts. QA these explicitly.
- Honest target: daily-drivable for developers/AI enthusiasts — browser + web apps + Flatpak + terminal + the agent runtime. Not for Photoshop, AutoCAD, or anticheat gamers, and the positioning should say so.

**Sources**: [NVIDIA open kernel modules default](https://www.phoronix.com/news/NVK-Status-Update-2025) · [NVIDIA open-gpu-kernel-modules](https://github.com/NVIDIA/open-gpu-kernel-modules) · [Secure Boot cert expiration (LWN)](https://lwn.net/Articles/1029767/) · [shim-review](https://github.com/rhboot/shim-review/blob/main/README.md) · [Bluefin Spring 2026 / Fedora 44](https://docs.projectbluefin.io/blog/bluefin-spring-2026/) · [Asahi feature support](https://asahilinux.org/docs/platform/feature-support/overview/) · [Framework Linux page](https://frame.work/linux) · [Framework 13 Pro Ubuntu-certified](https://www.omgubuntu.co.uk/2026/04/framework-13-pro-ubuntu-certified) · [SteamOS recovery/3.7+ on other AMD devices](https://gardinerbryant.com/heres-what-happened-when-i-installed-steamos-on-my-favorite-mini-pcs/)

//! `cargo xtask <command>` — the only supported way to build and boot the
//! kernel. Host tooling never ships in the image (purity charter standing
//! rule), and this crate deliberately has zero dependencies.
//!
//! Commands:
//!   build          compile nawa-kernel for x86_64-unknown-uefi (release)
//!   image          build + lay out target/esp as a UEFI system partition
//!   run [--gui]    boot the image in QEMU (serial on stdio)
//!   test           headless boot; assert serial hello + clean exit status
//!   check          enforce the framekernel rule: `unsafe` only in nawa/core
//!   disk           build target/i3mlos.img — the distributable disk image
//!   boot [--gui]   boot that image, the way anyone else would

mod disk;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, io::Read, thread};

const KERNEL_TARGET: &str = "x86_64-unknown-uefi";
const HELLO_LINE: &str = "hello from the i3ml kernel";
/// Every marker must appear on serial for the boot test to pass. Each one is
/// a law the kernel claims to enforce, asserted on every commit.
const MARKERS: [&str; 29] = [
    HELLO_LINE,
    "nawa: exit_boot_services ok",
    "int3: breakpoint handled",
    "heap: ok",
    "MiB usable",
    "apic timer armed",                       // the kernel has a heartbeat
    "delegated to agent",                     // agents spawn agents
    "refused to widen",                       // the attenuation law holds
    "parked awaiting approval",               // the irreversible waits for a human
    "suspended by its budget",                // budgets bind, spending never exceeds
    "own page tables live",                   // the kernel controls its own mappings
    "syscall gate armed",                     // ring 3 has exactly one way in
    "gate crossing(s) from ring 3",           // untrusted code reached the kernel
    "resident faulted reaching",              // and could not reach kernel memory
    "isolation is hardware now",              // the claim is the CPU's, not ours
    "refused /inbox-archive",                 // path prefixes end at boundaries
    "one yes, one action",                    // consent is bound and consumed
    "wasm: module decoded",                   // agents can be written, not compiled in
    "refused to load — imports invoke",        // the manifest is checked before the code runs
    "an agent is a module now",               // ...and then it runs
    "virtio device",                          // the kernel finds real hardware
    "bytes of real entropy",                  // randomness it did not invent
    "storage works",                          // and a disk that answers
    "entries persisted this boot",            // the journal reaches the disk
    "chain tip",                              // hash-chained, not just written
    "a model is a device behind the gate",     // the channel belongs to the kernel
    "model: answered in",                      // and a model actually answered
    "private class stayed on this machine",    // residency by routing, not by policy
    "the account is on the framebuffer",       // and a person can read all of it
];

/// True of the distributable image specifically: it must boot from the EFI
/// partition the builder wrote, and find the journal partition by its type
/// rather than by an assumed offset.
const IMAGE_MARKERS: [&str; 3] =
    [HELLO_LINE, "gpt: journal partition found", "entries persisted this boot"];

/// Only true on a boot that is not the first: the journal must be found,
/// verified, and recovered. A record that does not survive the machine is
/// not a record.
const REBOOT_MARKERS: [&str; 2] =
    ["entries recovered from disk, chain intact", "earliest surviving entry"];
/// isa-debug-exit: (0x10 << 1) | 1 — must match nawa_core::qemu::EXIT_SUCCESS.
const QEMU_SUCCESS_STATUS: i32 = 33;
const BOOT_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    let result = match command {
        "build" => build(),
        "image" => image().map(|_| ()),
        "run" => run(args.iter().any(|a| a == "--gui")),
        "test" => test(),
        "check" => check(),
        "disk" => build_disk().map(|_| ()),
        "boot" => boot_disk(args.iter().any(|a| a == "--gui"), false).map(|_| ()),
        "disk-test" => disk_test(),
        _ => {
            eprintln!("usage: cargo xtask <build|image|run [--gui]|disk|boot [--gui]|test|check>");
            std::process::exit(2);
        }
    };
    if let Err(message) = result {
        eprintln!("xtask error: {message}");
        std::process::exit(1);
    }
}

fn repo_root() -> PathBuf {
    // xtask always runs via cargo from the workspace; CARGO_MANIFEST_DIR is xtask/.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn build() -> Result<(), String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(repo_root())
        .args(["build", "-p", "nawa-kernel", "--target", KERNEL_TARGET, "--release"])
        .status()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if !status.success() {
        return Err("kernel build failed".into());
    }
    Ok(())
}

/// Lay out target/esp so QEMU's virtual-FAT drive is a bootable ESP:
/// firmware loads \EFI\BOOT\BOOTX64.EFI — which IS the kernel.
fn image() -> Result<PathBuf, String> {
    build()?;
    let root = repo_root();
    let efi = root.join("target").join(KERNEL_TARGET).join("release").join("nawa.efi");
    let esp = root.join("target").join("esp");
    let boot_dir = esp.join("EFI").join("BOOT");
    fs::create_dir_all(&boot_dir).map_err(|e| format!("mkdir {}: {e}", boot_dir.display()))?;
    let dest = boot_dir.join("BOOTX64.EFI");
    fs::copy(&efi, &dest).map_err(|e| format!("copy {} -> {}: {e}", efi.display(), dest.display()))?;
    println!("image: {}", esp.display());
    Ok(esp)
}

struct Firmware {
    code: PathBuf,
    vars: PathBuf,
}

/// Locate OVMF/EDK2 firmware (code + vars). Overridable via I3ML_OVMF_CODE /
/// I3ML_OVMF_VARS; otherwise the usual Homebrew/Linux locations.
fn find_firmware() -> Result<Firmware, String> {
    if let (Ok(code), Ok(vars)) = (env::var("I3ML_OVMF_CODE"), env::var("I3ML_OVMF_VARS")) {
        return Ok(Firmware { code: code.into(), vars: vars.into() });
    }
    let candidates = [
        ("/opt/homebrew/share/qemu/edk2-x86_64-code.fd", "/opt/homebrew/share/qemu/edk2-i386-vars.fd"),
        ("/usr/local/share/qemu/edk2-x86_64-code.fd", "/usr/local/share/qemu/edk2-i386-vars.fd"),
        ("/usr/share/qemu/edk2-x86_64-code.fd", "/usr/share/qemu/edk2-i386-vars.fd"),
        ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
        ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
        ("/usr/share/edk2/x64/OVMF_CODE.4m.fd", "/usr/share/edk2/x64/OVMF_VARS.4m.fd"),
    ];
    for (code, vars) in candidates {
        if Path::new(code).exists() && Path::new(vars).exists() {
            return Ok(Firmware { code: code.into(), vars: vars.into() });
        }
    }
    Err("UEFI firmware not found; install qemu (brew) or ovmf (apt), or set I3ML_OVMF_CODE/I3ML_OVMF_VARS".into())
}

/// CPU models the boot test sweeps. Both APIC access modes must work: real
/// hardware and modern QEMU offer x2APIC, but plenty of environments (older
/// QEMU builds, some CI images) expose only xAPIC MMIO — a path that shipped
/// once without coverage and broke CI, so it is a permanent test dimension.
const TEST_CPUS: [(&str, &str); 2] = [("x2apic", "max"), ("xapic", "max,-x2apic")];

fn qemu_command(esp: &Path, headless: bool, testing: bool) -> Result<Command, String> {
    qemu_command_with_cpu(esp, headless, testing, "max")
}

fn qemu_command_with_cpu(
    esp: &Path,
    headless: bool,
    testing: bool,
    cpu: &str,
) -> Result<Command, String> {
    let firmware = find_firmware()?;
    // Vars flash must be writable — give QEMU a scratch copy.
    let vars_copy = repo_root().join("target").join("ovmf-vars.fd");
    fs::copy(&firmware.vars, &vars_copy).map_err(|e| format!("copy vars: {e}"))?;

    let mut qemu = Command::new("qemu-system-x86_64");
    // The default QEMU CPU model advertises no APIC timer features at all, so
    // the kernel would fall back to running untimed; `max` turns them on.
    qemu.args(["-machine", "q35", "-cpu", cpu, "-m", "256M", "-nic", "none", "-serial", "stdio"]);
    qemu.arg("-drive").arg(format!("if=pflash,format=raw,readonly=on,file={}", firmware.code.display()));
    qemu.arg("-drive").arg(format!("if=pflash,format=raw,file={}", vars_copy.display()));
    qemu.arg("-drive").arg(format!("format=raw,file=fat:rw:{}", esp.display()));
    // Real devices for the kernel to drive: entropy it cannot invent, and a
    // disk that outlives the boot.
    qemu.args(["-device", "virtio-rng-pci"]);
    // The model channel: a console port wired to the host relay's socket.
    // The guest sees a device; the host side is where anything vendor-shaped
    // lives, and the kernel never holds a key.
    let socket = repo_root().join("target").join("i3ml-model.sock");
    // `reconnect-ms` on QEMU 10+, `reconnect` before it; neither is needed
    // when the relay is already listening, which is how the tests run it.
    qemu.arg("-chardev").arg(format!("socket,id=model,path={}", socket.display()));
    // A dedicated port, not the console: the firmware writes to port 0, and
    // a channel the firmware shares is a channel with someone else's bytes in
    // it.
    qemu.args([
        "-device",
        "virtio-serial-pci",
        "-device",
        "virtserialport,chardev=model,nr=1,name=os.i3ml.model",
    ]);
    let disk = repo_root().join("target").join("i3mlos-disk.img");
    if !disk.exists() {
        let _ = fs::write(&disk, vec![0u8; 16 * 1024 * 1024]);
    }
    qemu.arg("-drive").arg(format!("if=none,id=i3mldisk,format=raw,file={}", disk.display()));
    qemu.args(["-device", "virtio-blk-pci,drive=i3mldisk"]);
    if headless {
        qemu.args(["-display", "none"]);
    }
    if testing {
        qemu.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04", "-no-reboot"]);
    }
    Ok(qemu)
}

/// Build the image someone else can boot: one file, GPT-partitioned, with the
/// kernel on an EFI system partition and a partition for its journal.
fn build_disk() -> Result<PathBuf, String> {
    build()?;
    let root = repo_root();
    let kernel = root.join("target").join(KERNEL_TARGET).join("release").join("nawa.efi");
    let bytes = fs::read(&kernel).map_err(|error| format!("reading the kernel: {error}"))?;
    let output = root.join("target").join("i3mlos.img");
    disk::build(&output, &bytes, 128)?;
    println!("disk image: {} ({} MiB)", output.display(), 128);
    Ok(output)
}

/// Boot the distributable image exactly as a stranger would: one file, no
/// build tree, nothing from this repository mounted into it.
fn boot_disk(gui: bool, testing: bool) -> Result<String, String> {
    let image = repo_root().join("target").join("i3mlos.img");
    if !image.exists() {
        build_disk()?;
    }
    let firmware = find_firmware()?;
    let vars_copy = repo_root().join("target").join("ovmf-vars-boot.fd");
    fs::copy(&firmware.vars, &vars_copy).map_err(|error| format!("copy vars: {error}"))?;

    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.args(["-machine", "q35", "-cpu", "max", "-m", "256M", "-nic", "none", "-serial", "stdio"]);
    qemu.arg("-drive")
        .arg(format!("if=pflash,format=raw,readonly=on,file={}", firmware.code.display()));
    qemu.arg("-drive").arg(format!("if=pflash,format=raw,file={}", vars_copy.display()));
    qemu.arg("-drive").arg(format!("if=none,id=i3mlos,format=raw,file={}", image.display()));
    qemu.args(["-device", "virtio-blk-pci,drive=i3mlos"]);
    qemu.args(["-device", "virtio-rng-pci"]);
    if !gui {
        qemu.args(["-display", "none"]);
    }
    if !testing {
        println!("booting the image — serial follows:");
        let status = qemu.status().map_err(|error| format!("failed to spawn qemu: {error}"))?;
        println!("qemu exited: {status}");
        return Ok(String::new());
    }

    qemu.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04", "-no-reboot"]);
    qemu.stdout(Stdio::piped()).stderr(Stdio::inherit()).stdin(Stdio::null());
    let (serial, status) = capture(qemu)?;
    if status.code() != Some(QEMU_SUCCESS_STATUS) {
        return Err(format!("image boot: expected exit {QEMU_SUCCESS_STATUS}, got {status}"));
    }
    Ok(serial)
}

/// The artifact test: build the image a stranger would download, boot it, and
/// boot it again. Nothing from this build tree is mounted into it — if the
/// image is wrong, this is where that shows.
fn disk_test() -> Result<(), String> {
    let mut relay = start_relay()?;
    let image = repo_root().join("target").join("i3mlos.img");
    let _ = fs::remove_file(&image);
    build_disk()?;

    println!("=== image boot 1: does the artifact boot at all? ===");
    let first = boot_disk(false, true)?;
    for marker in IMAGE_MARKERS {
        if !first.contains(marker) {
            return Err(format!("image boot 1: serial output missing \"{marker}\""));
        }
    }
    println!("  ok: booted from its own EFI partition and found its journal partition");

    println!("=== image boot 2: does its journal survive? ===");
    let second = boot_disk(false, true)?;
    for marker in REBOOT_MARKERS {
        if !second.contains(marker) {
            return Err(format!("image boot 2: serial output missing \"{marker}\""));
        }
    }
    println!("  ok: the journal in the image outlived the machine");
    let _ = relay.kill();
    println!("IMAGE TEST OK");
    Ok(())
}

fn run(gui: bool) -> Result<(), String> {
    let esp = image()?;
    let mut qemu = qemu_command(&esp, !gui, false)?;
    println!("booting i3mlOS in QEMU{} — serial follows:", if gui { " (window)" } else { "" });
    let status = qemu.status().map_err(|e| format!("failed to spawn qemu: {e}"))?;
    println!("qemu exited: {status}");
    Ok(())
}

fn test() -> Result<(), String> {
    let esp = image()?;
    // The model relay runs for the duration: without it the machine has
    // nothing to think with, and the test says so rather than skipping it.
    let mut relay = start_relay()?;
    // Start from a blank disk so the first boot is genuinely the first.
    let disk = repo_root().join("target").join("i3mlos-disk.img");
    let _ = fs::remove_file(&disk);

    for (name, cpu) in TEST_CPUS {
        println!("=== boot test: {name} ({cpu}) ===");
        let serial = boot_once(&esp, cpu)?;
        for marker in MARKERS {
            if !serial.contains(marker) {
                return Err(format!("[{cpu}] serial output missing \"{marker}\""));
            }
        }
    }

    // The reboot test. Everything above ran against a disk that started
    // empty; this boot must find what the previous ones committed — that is
    // the whole point of a journal, and it cannot be checked in one run.
    println!("=== reboot test: does the journal outlive the machine? ===");
    let serial = boot_once(&esp, TEST_CPUS[0].1)?;
    for marker in REBOOT_MARKERS {
        if !serial.contains(marker) {
            return Err(format!("after reboot, serial output missing \"{marker}\""));
        }
    }
    println!("  ok: the journal was recovered and verified");

    let _ = relay.kill();
    println!("BOOT TEST OK: all {} markers on {} CPU models, plus recovery", MARKERS.len(), TEST_CPUS.len());
    Ok(())
}

/// Start the host end of the model channel. It answers deterministically
/// unless I3ML_MODEL_ENDPOINT and I3ML_MODEL_KEY are set, so the whole path —
/// capability, budget, channel, journal — is exercised with no network and no
/// secret.
fn start_relay() -> Result<std::process::Child, String> {
    let root = repo_root();
    let socket = root.join("target").join("i3ml-model.sock");
    let _ = fs::remove_file(&socket);
    let child = Command::new("python3")
        .arg(root.join("host").join("relay").join("relay.py"))
        .arg(&socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("starting the model relay: {error}"))?;
    // Wait for it to bind, so QEMU has something to connect to.
    for _ in 0..200 {
        if socket.exists() {
            return Ok(child);
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err("the model relay never bound its socket".into())
}

/// Boot once and return everything the kernel said on the serial port.
fn boot_once(esp: &Path, cpu: &str) -> Result<String, String> {
    let mut qemu = qemu_command_with_cpu(esp, true, true, cpu)?;
    qemu.stdout(Stdio::piped()).stderr(Stdio::inherit()).stdin(Stdio::null());
    let (serial, status) = capture(qemu)?;

    println!("--- serial ---");
    print!("{serial}");
    println!("--------------");

    if status.code() != Some(QEMU_SUCCESS_STATUS) {
        return Err(format!("[{cpu}] expected qemu exit status {QEMU_SUCCESS_STATUS}, got {status}"));
    }
    println!("  ok: clean exit ({QEMU_SUCCESS_STATUS})");
    Ok(serial)
}

/// Run QEMU to completion, returning everything it said and how it ended.
/// A machine that will not stop is killed rather than waited on forever.
fn capture(mut qemu: Command) -> Result<(String, std::process::ExitStatus), String> {
    let start = Instant::now();
    let mut child = qemu.spawn().map_err(|e| format!("failed to spawn qemu: {e}"))?;
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let reader = thread::spawn(move || {
        let mut serial = String::new();
        let _ = stdout.read_to_string(&mut serial);
        serial
    });

    let status = loop {
        match child.try_wait().map_err(|e| format!("wait: {e}"))? {
            Some(status) => break status,
            None if start.elapsed() > BOOT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("boot test timed out after {}s", BOOT_TIMEOUT.as_secs()));
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    };
    let serial = reader.join().unwrap_or_default();
    Ok((serial, status))
}

/// Framekernel rule: unsafe *operations* may appear only under nawa/core.
/// Everything else in kernel space (nawa/*, libs/, yard/) must be safe Rust.
/// Entry-symbol attributes (`unsafe(export_name)` / `unsafe(no_mangle)`) are
/// linker-hazard markers, not operations, and are permitted with a scoped
/// `#[allow(unsafe_code)]` — the compiler still gates them per crate.
fn check() -> Result<(), String> {
    const UNSAFE_OPERATIONS: [&str; 6] =
        ["unsafe {", "unsafe{", "unsafe fn", "unsafe impl", "unsafe trait", "unsafe extern"];
    let root = repo_root();
    let mut violations = Vec::new();
    for dir in ["nawa", "libs", "yard"] {
        walk_rs(&root.join(dir), &mut |path| {
            if path.starts_with(root.join("nawa").join("core")) {
                return;
            }
            let source = fs::read_to_string(path).unwrap_or_default();
            for (index, line) in source.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if UNSAFE_OPERATIONS.iter().any(|op| code.contains(op)) {
                    violations.push(format!("{}:{}: {}", path.display(), index + 1, line.trim()));
                }
            }
        });
    }
    if violations.is_empty() {
        println!("CHECK OK: `unsafe` is confined to nawa/core (framekernel rule)");
        Ok(())
    } else {
        for v in &violations {
            eprintln!("unsafe outside nawa/core: {v}");
        }
        Err(format!("{} framekernel violation(s)", violations.len()))
    }
}

fn walk_rs(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            walk_rs(&path, visit);
        } else if path.extension().is_some_and(|e| e == "rs") {
            visit(&path);
        }
    }
}

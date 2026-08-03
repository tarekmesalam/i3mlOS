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

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, io::Read, thread};

const KERNEL_TARGET: &str = "x86_64-unknown-uefi";
const HELLO_LINE: &str = "hello from the i3ml kernel";
/// Every marker must appear on serial for the boot test to pass. Each one is
/// a law the kernel claims to enforce, asserted on every commit.
const MARKERS: [&str; 23] = [
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
];
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
        _ => {
            eprintln!("usage: cargo xtask <build|image|run [--gui]|test|check>");
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
    for (name, cpu) in TEST_CPUS {
        println!("=== boot test: {name} ({cpu}) ===");
        boot_once(&esp, cpu)?;
    }
    println!("BOOT TEST OK: all {} markers on {} CPU models", MARKERS.len(), TEST_CPUS.len());
    Ok(())
}

fn boot_once(esp: &Path, cpu: &str) -> Result<(), String> {
    let mut qemu = qemu_command_with_cpu(esp, true, true, cpu)?;
    qemu.stdout(Stdio::piped()).stderr(Stdio::inherit()).stdin(Stdio::null());

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

    println!("--- serial ---");
    print!("{serial}");
    println!("--------------");

    for marker in MARKERS {
        if !serial.contains(marker) {
            return Err(format!("[{cpu}] serial output missing \"{marker}\""));
        }
    }
    if status.code() != Some(QEMU_SUCCESS_STATUS) {
        return Err(format!("[{cpu}] expected qemu exit status {QEMU_SUCCESS_STATUS}, got {status}"));
    }
    println!("  ok: all {} markers found, clean exit ({QEMU_SUCCESS_STATUS})", MARKERS.len());
    Ok(())
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

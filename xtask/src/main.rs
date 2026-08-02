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

fn qemu_command(esp: &Path, headless: bool, testing: bool) -> Result<Command, String> {
    let firmware = find_firmware()?;
    // Vars flash must be writable — give QEMU a scratch copy.
    let vars_copy = repo_root().join("target").join("ovmf-vars.fd");
    fs::copy(&firmware.vars, &vars_copy).map_err(|e| format!("copy vars: {e}"))?;

    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.args(["-machine", "q35", "-m", "256M", "-nic", "none", "-serial", "stdio"]);
    qemu.arg("-drive").arg(format!("if=pflash,format=raw,readonly=on,file={}", firmware.code.display()));
    qemu.arg("-drive").arg(format!("if=pflash,format=raw,file={}", vars_copy.display()));
    qemu.arg("-drive").arg(format!("format=raw,file=fat:rw:{}", esp.display()));
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
    let mut qemu = qemu_command(&esp, true, true)?;
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

    if !serial.contains(HELLO_LINE) {
        return Err(format!("serial output missing \"{HELLO_LINE}\""));
    }
    if status.code() != Some(QEMU_SUCCESS_STATUS) {
        return Err(format!("expected qemu exit status {QEMU_SUCCESS_STATUS}, got {status}"));
    }
    println!("BOOT TEST OK: found \"{HELLO_LINE}\", clean exit ({QEMU_SUCCESS_STATUS})");
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

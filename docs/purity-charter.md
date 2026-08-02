# The Purity Charter

**Standing rule: third-party code may appear in build/test/CI tooling
(differential tests, fuzz corpora, interop suites) but never in the shipped
image.** `cargo xtask check` additionally enforces the framekernel rule:
`unsafe` is legal only inside `nawa/core`.

| Tier | Contents |
|---|---|
| **1 — Write now, from scratch** | UEFI-app kernel + boot path, frame/heap allocators, scheduler + executor, all VirtIO drivers (PCI + MMIO transports), filesystem + SIJIL on-disk format, TCP/IP stack, HTTP/1.1, TLS 1.3 state machine, TTF/OTF parser, UAX #9 bidi, Arabic shaper (terminal-grade first), 2D compositor, WASM interpreter (non-JIT), QOI/PNG decoders, pvclock/kvmclock time reader |
| **2 — Vendor-then-replace (the sole exception, public)** | Crypto primitives + X.509 path validation (rustls/ring/webpki), confined to the net-broker task in the yard — never linked into NAWA, AMAN, or any agent. Replace triggers, both required: (a) own constant-time primitives pass Wycheproof + BoGo; (b) a funded external cryptographic audit. Declared permanent-until-funded, publicly. |
| **3 — Accepted forever (boundary and data, not code)** | rustc/LLVM toolchain, machine firmware (UEFI), QEMU/Firecracker as dev/deploy hosts, AI model weights/APIs, Unicode data files (UCD, ArabicShaping.txt, BidiTest.txt) + conformance suites, OpenType/IETF/OASIS specs, Mozilla CA store as versioned data with an update mechanism, fonts (Amiri/Noto Naskh — content, like weights) |

Full rationale, the explicit TLS and Arabic-shaping decisions, and the claims
ladder live in [MASTER_PLAN.md](../MASTER_PLAN.md) §2.

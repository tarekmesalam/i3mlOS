# Purity Precedents Brief — where "from scratch" projects drew the line, and where i3mlOS should

## 1. SerenityOS — purity as culture engine

Started October 2018 by Andreas Kling as a post-rehab therapy project with one rule: **write everything yourself** — own libc, LibCore, LibGfx (2D graphics + font rendering), own TCP/IP stack, LibCrypto/LibTLS instead of OpenSSL, and eventually LibWeb/LibJS, a browser and JS engine from nothing. In ~4 years one person plus volunteers went from zero to a self-hosting desktop with a working browser; the project drew 1,000+ contributors. The purity *was* the growth strategy: "no third-party code" made every subsystem a greenfield invitation, the monthly videos remarkable, and the culture legible. Two lessons cut both ways: (a) a disciplined solo dev demonstrably CAN write TLS, TCP/IP, font rendering, and a browser; (b) when Kling pivoted from artifact to product, **Ladybird (spun off 2022, fully forked June 2024) dropped the purity rule** and adopted vendored libraries (Skia, ICU, ffmpeg…) — and needed a 501(c)(3) with $1M from GitHub co-founder Chris Wanstrath, sponsors (FUTO, Shopify, Cloudflare, Proton) and ~7 paid engineers to target a 2026 alpha. Purity built the movement; shipping required repealing it *for the browser* — an app-scale problem i3mlOS deliberately doesn't have.

## 2. TempleOS — the outer extreme

Terry Davis, alone, ~2003–2013: own compiler (HolyC), own kernel, filesystem, graphics, ~120K lines. Proof of the solo ceiling: one person can own the *entire* stack including the compiler — at the price of no networking, no security model, frozen 640×480 hardware scope, and a bus factor of one. Take from it the ceiling estimate (a coherent solo OS is ~100–200K disciplined lines) and the warning: "own compiler" consumes the decade that should go to the actual idea. i3mlOS's accepted rustc/LLVM boundary is the correct anti-TempleOS decision.

## 3. The pragmatists — Redox, Haiku, Ladybird

**Redox** (2015–) writes its own microkernel and relibc but pragmatically reuses ecosystem crates — most notably **smoltcp** for its entire netstack — and after 11 years its milestone is "a few developers on real hardware by end-2026" (see `/Users/tarekmesalam/Projects/i3mlOS/docs/research/06-kernel-independence-endgame.md`). Crate reuse did not buy Redox speed where it mattered (drivers). **Haiku** (2001–) reused POSIX and ported OpenSSL/ffmpeg/WebKit and still took 20+ years to R1 betas on donation funding. Conclusion: **reuse saves single-digit engineer-years; the schedule is dominated by hardware and polish, which i3mlOS's virtio-only decision already amputates.** So purity is cheaper for us than base rates suggest.

## 4. The hard-problem menu

| Problem | From-scratch effort (solo) | Risk / precedent |
|---|---|---|
| **TCP/IP** (vs smoltcp) | 6–12 mo to working TCP, +years of edge-case maturity | Serenity did it; smoltcp (~40K LoC) shows scope. Differential-test against Linux host + smoltcp *in CI only*. Feasible. |
| **TLS 1.3** | State machine 6–9 mo; the danger is not the RFC 8446 logic but crypto side channels | Serenity precedent exists; interop was long flaky. Options: vendor rustls/ring temporarily; write own against BoringSSL's BoGo interop suite; or terminate TLS in a **sidecar net-broker component** so the exception never touches the OS image. |
| **Crypto primitives** | ChaCha20-Poly1305, X25519, Ed25519, SHA-2 are *designed* to be reimplementable constant-time; AES-GCM is not (needs AES-NI intrinsics) | Malpractice risk is real: passing Wycheproof + RFC vectors proves correctness, not side-channel safety. Nuance to flag as an explicit purity decision: code *generated* from formally verified specs (fiat-crypto emits Rust) — from-scratch-adjacent. |
| **Text rendering: UAX #9 bidi + Arabic joining/shaping** | Bidi: 2–3 mo — Unicode ships `BidiTest.txt` conformance files. Terminal-grade Arabic: joining classes are literally a data file (`ArabicShaping.txt`); contextual forms on a monospace grid is 1–2 mo (precedent: mlterm). **Naskh-grade** (GSUB/GPOS ligatures, marks) is the hard 80%: rustybuzz took years to pass 2,221/2,252 HarfBuzz tests; allsorts (Prince XML) proves a small team can write an independent shaper | **Feasible and strategically core.** An Arabic-first OS whose Arabic shaper is its own, validated against the HarfBuzz test corpus, is the single most credible purity flex available to this project. Scope to Arabic+Latin first, not all scripts. |
| **TTF/OTF parsing** | 2–4 mo | ttf-parser is essentially one author (RazrFalcon). Well-specified; fonts are attack surface → safe Rust + fuzzing. Easy win. |
| **2D compositor over virtio-gpu** | 4–8 mo software rasterizer + damage-tracking compositor | Serenity's WindowServer precedent; virtio-gpu removes driver hell. Feasible. |
| **Image/audio codecs** | QOI: days. PNG(+inflate): 1–2 mo. JPEG: 2–3 mo. Opus: hard — defer audio decode | Serenity wrote all decoders. High attack surface → ideal safe-Rust showcase. |
| **WASM interpreter as sandboxed app/tool format** | 3–6 mo, 10–20K lines, non-JIT | Strong precedents: Serenity's LibWasm, wasm3 (~1 principal author), wasmi, Wasmtime's Pulley. The spec ships an official test suite — correctness is checkable. 10–50× slower than JIT is fine for IO-bound agent tools. **Do it; it's the right app format** (Nebulet, Hyperlight-Wasm, Extism as ecosystem precedents). |
| **Own compiler** | Decade-scale (TempleOS, Oberon) | Note only. Revisit at year 10, never before. |

## 5. The supply-chain story

Zero third-party crates in the shipped image is a *marketable security property*, not vanity: no transitive-dependency CVEs, no xz-utils-style backdoor surface, no crates.io typosquatting, `cargo vet` unnecessary — **the entire TCB auditable by one person**, which matters uniquely for an OS whose schedulable unit is an autonomous agent (the thing enforcing AMAN cannot itself be trojaned via a dep). Be honest in the telling: CVE-free ≠ bug-free; your own crypto bug has no advisory feed. The mitigation is the story's second half: every parser fuzzed, every crypto function Wycheproof-tested, external audit before any security claim.

## 6. Recommended purity charter

**Standing rule: third-party code may appear in build/test/CI tooling (differential testing, fuzz corpora, interop suites) but never in the shipped image.**

**Tier 1 — Write now (from scratch, day one):** kernel, scheduler, virtio drivers, filesystem + SIJIL journal, TCP/IP stack, TTF parser, 2D compositor, UAX #9 bidi + Arabic shaper (terminal-grade first, naskh-grade the flagship goal), QOI/PNG/JPEG decoders, WASM interpreter, TLS 1.3 state machine.

**Tier 2 — Vendor-then-replace (the single exception, named triggers):** crypto primitives + TLS cipher operations — vendor rustls/ring, contained inside the net-broker sidecar so it never links into kernel or AMAN. Replace when: (a) own primitives pass Wycheproof + BoGo, AND (b) budget exists for one external cryptographic audit. Until both, the exception stands and is documented, not hidden.

**Tier 3 — Accept forever (data and boundary, not code):** rustc/LLVM, UEFI, QEMU/Firecracker hosts, model weights/APIs, Unicode data files (UCD, ArabicShaping.txt) and conformance suites, OpenType/IETF specs, the Mozilla CA trust store (as data), and fonts themselves (Amiri/Noto Naskh — content, like weights).

This charter keeps the SerenityOS cultural engine, avoids the TempleOS compiler trap, contains the one genuinely dangerous item (crypto) behind a named, temporary, auditable exception — and makes the Arabic text stack the place where purity and positioning are the same thing.

Sources: [Ladybird README (SerenityOS libs)](https://github.com/SerenityOS/serenity/blob/master/Ladybird/README.md) · [LWN: Ladybird spreads its wings](https://lwn.net/Articles/976822/) · [Ladybird (Wikipedia)](https://en.wikipedia.org/wiki/Ladybird_(web_browser)) · [ladybird.org](https://ladybird.org/) · [Ladybird Initiative announcement](https://ladybird.org/posts/announcement/) · [Hackaday: SerenityOS/Ladybird fork](https://hackaday.com/2024/07/02/fork-ladybird-browser-and-serenityos-to-go-separate-ways/) · [Redox netstack (smoltcp)](https://github.com/redox-os/netstack) · [rustybuzz](https://github.com/harfbuzz/rustybuzz) · [harfrust](https://github.com/harfbuzz/harfrust) · [rustls](https://crates.io/crates/rustls)

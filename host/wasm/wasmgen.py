#!/usr/bin/env python3
"""Emit the demo agent module, as bytes, into nawa/kernel/src/toolmod.rs.

Host tooling: a five-instruction WebAssembly assembler. The module it builds
is the first agent i3mlOS runs that is *written* rather than compiled in.
"""

def uleb(v):
    out = bytearray()
    while True:
        b = v & 0x7f
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)

def sleb(v):
    out = bytearray()
    while True:
        b = v & 0x7f
        v >>= 7
        if (v == 0 and not (b & 0x40)) or (v == -1 and (b & 0x40)):
            out.append(b)
            return bytes(out)
        out.append(b | 0x80)

def vec(items):
    return uleb(len(items)) + b"".join(items)

def section(sid, payload):
    return bytes([sid]) + uleb(len(payload)) + payload

def name(text):
    raw = text.encode()
    return uleb(len(raw)) + raw

# --- the module ---------------------------------------------------------
# (import "i3ml" "invoke"  (func (param i32) (result i32)))   ; index 0
# (import "i3ml" "journal" (func (param i64) (result i32)))   ; index 1
# (func (export "run") (result i32)
#   ;; journal(0x1391)
#   i64.const 0x1391  call 1  drop
#   ;; three reads through the granted capability, counting successes
#   (local i32 i32)
#   loop over 3: invoke(0) ; if result == 0 then count += 1
#   count)
type_invoke  = bytes([0x60]) + uleb(1) + bytes([0x7f]) + uleb(1) + bytes([0x7f])
type_journal = bytes([0x60]) + uleb(1) + bytes([0x7e]) + uleb(1) + bytes([0x7f])
type_run     = bytes([0x60]) + uleb(0) + uleb(1) + bytes([0x7f])

types = section(1, vec([type_invoke, type_journal, type_run]))
imports = section(2, vec([
    name("i3ml") + name("invoke")  + bytes([0x00]) + uleb(0),
    name("i3ml") + name("journal") + bytes([0x00]) + uleb(1),
]))
functions = section(3, vec([uleb(2)]))          # one function, type 2
exports = section(7, vec([name("run") + bytes([0x00]) + uleb(2)]))

code = bytearray()
# journal(0x1391); drop the result
code += bytes([0x42]) + sleb(0x1391)            # i64.const 0x1391
code += bytes([0x10]) + uleb(1)                 # call 1 (journal)
code += bytes([0x1a])                           # drop
# local 0 = attempts remaining (3), local 1 = successes
code += bytes([0x41]) + sleb(3) + bytes([0x21, 0x00])
code += bytes([0x41]) + sleb(0) + bytes([0x21, 0x01])
code += bytes([0x02, 0x40])                     # block
code += bytes([0x03, 0x40])                     #   loop
code += bytes([0x20, 0x00, 0x45, 0x0d, 0x01])   #     if attempts == 0: br 1
code += bytes([0x41]) + sleb(0)                 #     i32.const 0  (arg)
code += bytes([0x10]) + uleb(0)                 #     call 0 (invoke)
code += bytes([0x45])                           #     i32.eqz  -> 1 if ok
code += bytes([0x20, 0x01, 0x6a, 0x21, 0x01])   #     successes += that
code += bytes([0x20, 0x00, 0x41]) + sleb(1) + bytes([0x6b, 0x21, 0x00])  # attempts -= 1
code += bytes([0x0c, 0x00])                     #     br 0
code += bytes([0x0b])                           #   end loop
code += bytes([0x0b])                           # end block
code += bytes([0x20, 0x01])                     # successes

body = uleb(1) + uleb(2) + bytes([0x7f]) + bytes(code) + bytes([0x0b])  # one group of two i32 locals
code_section = section(10, vec([uleb(len(body)) + body]))

module = b"\0asm\x01\0\0\0" + types + imports + functions + exports + code_section

out = [
    "// The first agent i3mlOS runs that is *written* rather than compiled in:",
    "// a WebAssembly module. Its imports are its manifest —",
    "//",
    '//     (import "i3ml" "invoke"  ...)   ; needs a capability, bound by the kernel',
    '//     (import "i3ml" "journal" ...)   ; needs none: the record is the kernel\'s',
    "//",
    "// It journals a greeting, then exercises its one capability three times and",
    "// returns how many succeeded — so a refusal is visible in its result, not",
    "// just in the log. Regenerate with host/wasm/wasmgen.py.",
    "//",
    "// Included verbatim by libs/wasm/tests/agent_module.rs, which is why the",
    "// header is plain comments: an inner doc comment cannot be include!()d.",
    "",
    f"pub static MODULE: [u8; {len(module)}] = [",
]
for i in range(0, len(module), 16):
    out.append("    " + ", ".join(f"0x{b:02x}" for b in module[i:i+16]) + ",")
out += [
    "];",
    "",
    "/// The greeting the module journals — the same marker the ring 3 resident",
    "/// uses, so the two paths are comparable in the log.",
    "pub const GREETING: u64 = 0x1391;",
]
import os
target = os.path.join(os.path.dirname(__file__), "../../nawa/kernel/src/toolmod.rs")
target = "/Users/tarekmesalam/Projects/i3mlOS/nawa/kernel/src/toolmod.rs"
with open(target, "w") as f:
    f.write("\n".join(out) + "\n")
print(f"emitted toolmod.rs: {len(module)} bytes")

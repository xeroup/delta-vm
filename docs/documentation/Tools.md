## Delta Assembly - Tools

---

### dvm

The Delta VM - interpreter and compiler driver.

```
dvm <file> --entry <func> [options]
```

`<file>` may be a `.ds` source file or a `.dc` compiled bytecode file.

**Options:**

| Flag | Description |
|---|---|
| `--entry <n>` | Function to call as the program entry point (required) |
| `--bench` | Print load and run timings to stderr after execution |
| `--compile` | Compile instead of interpret |
| `--emit <format>` | Output format when `--compile` is set (default: `obj`) |
| `-o <path>` | Output file path (default: derived from input filename) |

**Emit formats:**

| Value | Output | Description |
|---|---|---|
| `exe` | native binary | Linked executable (`.exe` on Windows, no extension on Linux/macOS). Requires system `cc` or `ld`. |
| `obj` | `.o` | Relocatable ELF/Mach-O/COFF object file |

**Examples:**

```sh
# interpret
dvm program.ds --entry main

# compile to executable
dvm program.ds --entry main --compile --emit exe -o program

# compile to object file
dvm program.ds --entry main --compile --emit obj -o program.o

# benchmark interpreter
dvm program.ds --entry main --bench
```

**Exit code:** the integer returned by the entry function.

---

### das

The Delta disassembler - converts `.dc` bytecode back to human-readable assembly.

```
das <file> [options]
```

`<file>` may be `.dc` or `.ds` (compiles first, then disassembles - useful for a round-trip check).

**Options:**

| Flag | Description |
|---|---|
| `--info` | Print file summary only (function table, data section, externs) |
| `-f <n>` | Disassemble a single named function |
| `--hex` | Show raw instruction bytes alongside mnemonics |

**Examples:**

```sh
# full disassembly
das program.dc

# summary
das program.dc --info

# one function with hex
das program.dc -f main --hex

# round-trip: assemble and immediately disassemble
das program.ds
```

**Output format:**

```
; delta bytecode - 2 function(s)
;
; data:
; [0] str "Hello, world!\n"

fn main (regs=2, params=0):
  0000: load.p r0, data[0] ; "Hello, world!\n"
  0008: print.p r0
  000c: load.i r1, 0
  0014: ret r1
```

Jump targets are resolved to labels:

```
fn fib (regs=4, params=1):
  0000: load.i r1, 2
  0008: lt.i r2, r0, r1
  000c: jmp.if r2, .L11 ; 0x004c
  ...
  .L11:
  004c: ret r0
```

---

### Error messages

Both tools print diagnostics to stderr in the format:

```
error: <message> (at line:col)
warning: <message> (at line:col)
```

Exit code is `1` on any error.

## Delta Virtual Machine

Delta is a register-based bytecode virtual machine and compiler toolchain written in Rust. It consists of an assembler, a bytecode interpreter, a native code compiler (via Cranelift), and a disassembler - all in a single self-contained binary.

The primary goal is to provide a small, hackable target platform for experimenting with language implementation: write a compiler that emits `.ds` assembly, hand it to `dvm`, and get either interpreted execution or a native ELF/Mach-O/PE binary.


### Features

- **Register-based VM** - fixed-width 64-bit registers, simple 4-format instruction encoding
- **Typed assembly** (`int`, `bool`, `float`, `char`, `ptr`) with a static checker and linter
- **Native code generation** - Cranelift backend emits optimised machine code for x86-64 and arm64
- **System linker** - `--emit exe` produces a runnable binary using the system `cc`/`ld` (no external toolchain to install separately)
- **Disassembler** - `das` reconstructs human-readable assembly from `.dc` bytecode, with resolved jump labels and inline data comments
- **60+ opcodes** - integer and float arithmetic, comparisons, casts, strings, arrays, heap allocation, I/O, time


### Crate layout

```
delta-format/ binary format, opcode table, instruction encoding/decoding
delta-asm/ lexer -> parser -> AST -> checker -> linter -> resolver
delta-codegen/ AST -> .dc bytecode
delta-cranelift/ .dc -> Cranelift IR -> native object/exe
tools/dvm/ interpreter + compile driver
tools/das/ disassembler
```


### Building

Requires **Rust stable** (1.75+). No external libraries needed - Cranelift is a pure-Rust crate.

```sh
cargo build --release
```

To produce native executables with `--emit exe`, the system linker must be available:
- **Linux/macOS** - `cc` (comes with GCC or Xcode CLT)
- **Windows** - `link.exe` (MSVC) or `gcc` (MinGW)


### Installation

Build the release binaries first:

```sh
cargo build --release
```

Then place the installer next to the compiled binaries and run it:

```sh
# Linux / macOS (writes to /usr/local/bin)
sudo ./target/release/installer

# Windows (writes to C:\Program Files\Delta-VM and adds it to your user PATH)
.\target\release\installer.exe
```

The installer copies `das` and `dvm` (or `.exe` on Windows) from its own directory
to the target location. On Windows the destination is added to `HKCU\Environment\PATH`,
so no admin rights are required - just reopen your terminal after installation.

To uninstall, delete the installed binaries manually:

```sh
# Linux / macOS
sudo rm /usr/local/bin/das /usr/local/bin/dvm

# Windows (PowerShell)
Remove-Item "C:\Program Files\Delta-VM" -Recurse
```


### Usage

```sh
# interpret a .ds source file
dvm program.ds --entry main

# compile to a native executable
dvm program.ds --entry main --compile --emit exe -o program

# compile to a relocatable object file
dvm program.ds --entry main --compile --emit obj -o program.o

# run with timing
dvm program.ds --entry main --bench

# disassemble bytecode
das program.dc
das program.ds --hex # show raw bytes alongside mnemonics
das program.dc --info # file summary only
das program.dc -f main # one function only
```


### Assembly example

```asm
.section code

.func fib(int r0) -> int
    int r1
    int r2
    int r3
    load r1, 2
    lt r2, r0, r1
    jmpif r2, base
    load r1, 1
    sub r2, r0, r1
    call r3, fib, r2
    load r1, 2
    sub r2, r0, r1
    call r1, fib, r2
    add r1, r3, r1
    ret r1
base
    ret r0
.endfunc

.func main() -> int
    int r0
    int r1
    load r0, 10
    call r1, fib, r0
    printint r1
    load r0, 0
    ret r0
.endfunc
```

```sh
$ dvm fib.ds --entry main
55

$ dvm fib.ds --entry main --compile --emit exe -o fib && ./fib
55
```

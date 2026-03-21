## Delta Assembly - Overview

Delta assembly (`.ds`) is the text-based source format for the Delta Virtual Machine. A `.ds` file is compiled by `dvm` into `.dc` bytecode, which is then either interpreted by the VM or compiled to a native binary via Cranelift.

The language is intentionally minimal: it sits one level above raw bytecode, giving you named registers, labels, a type system, and readable mnemonics - without any expression syntax, operator precedence, or control structures beyond jumps.

---

### Design principles

- **Register-based.** All values live in named registers. There is no stack.
- **Explicitly typed.** Every register has a declared type: `int`, `bool`, `float`, `char`, or `ptr`. The assembler enforces types statically. `bool` is an alias for `int` (0/1) and is interchangeable with it.
- **Flat.** Functions cannot be nested. Labels are local to their function. There is no module system.
- **No implicit behaviour.** Arithmetic does not coerce types. Passing a `float` register where `int` is expected is an error at assemble time.
- **Immediates anywhere.** Any source operand may be a literal - the assembler generates the necessary load automatically.
- **First-class function pointers.** Functions can be stored in `ptr` registers and called indirectly.

---

### Instruction categories

| Category | Mnemonics |
|---|---|
| Integer arithmetic | `add sub mul div mod pow neg abs` |
| Float arithmetic | `add sub mul div modf powf negf absf sqrt` |
| Comparisons | `eq ne lt le gt ge` |
| Type casts | `itof ftoi itoc ctoi ptoi` |
| Load | `load` |
| Control flow | `jmp jmpif jmpifnot ret` |
| Calls | `call call.void call.ext call.ext.void` |
| Function pointers | `func.ptr call.ptr call.ptr.void` |
| Memory | `alloc free store read` |
| Arrays | `arr.new arr.get arr.set arr.len arr.free` |
| Bitwise | `and or xor not shl shr` |
| Strings | `strlen streq charat upper lower itos ftos` |
| Output | `printint printfloat printchar printptr print` |
| Input | `readint readfloat readchar readline` |
| Time | `timens timems timemonons` |
| Control | `panic` |

---

### Pipeline

```
source.ds
    |
    v
  Lexer        tokenise text into tokens
    |
    v
  Parser       build AST (FuncDecl, Instruction, Operand, ...)
    |
    v
  Resolver     resolve labels -> byte offsets, data names -> indices
    |
    v
  Checker      static type checking of register usage and call signatures
    |
    v
  Linter       warnings: unused registers, unreachable code after jmp/ret
    |
    v
  Codegen      emit .dc bytecode
    |
    v
  dvm / Cranelift   interpret or compile to native
```

---

### File structure

A `.ds` file consists of three optional sections in any order:

```
extern declarations      .extern name(types) -> type
                         .extern name(types, ...) -> type   ; variadic
data section             .section data
code section             .section code  (default - may be omitted)
```

Sections may appear multiple times; their contents are merged. The `code` section keyword itself is optional - `.func` declarations at the top level are always accepted.

See [Structure.md](Structure.md) for the full layout.

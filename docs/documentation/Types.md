## Delta Assembly - Type System

Delta assembly is statically typed at the register level. Every register has a type declared at the top of the function body. The assembler's checker verifies that instructions receive operands of the correct types and that call arguments match the callee's signature.

Type errors are reported before any code is generated.

---

### The five types

| Type | Description | Typical use |
|---|---|---|
| `int` | 64-bit signed integer | counters, indices, bitwise operations |
| `bool` | boolean (alias for `int`) | flags, conditions, comparison results |
| `float` | 64-bit IEEE 754 double | floating-point computation |
| `char` | Unicode scalar value (≤ 21 bits used) | single characters |
| `ptr` | 64-bit address | heap memory, strings, arrays, data section references, function pointers |

`void` exists as a return type annotation only. No register may be declared `void`.

---

### `bool` and `int` compatibility

`bool` is an alias for `int` at the storage level - both occupy a 64-bit slot and use 0 for false, 1 for true. They are fully interchangeable in all instructions and call signatures. The distinction is semantic only: use `bool` to document intent.

```asm
.func is_positive(int n) -> bool
    bool result
    gt result, n, 0
    ret result
.endfunc
```

Comparison instructions (`eq`, `ne`, `lt`, `le`, `gt`, `ge`) may write their result into either an `int` or `bool` register.

---

### How types are checked

The checker builds a map of `register name -> type` from the function's parameter list and local declarations. It then walks every instruction and verifies:

**Arithmetic (`add`, `sub`, `mul`, `div`, `mod`, `pow`):**
Both source operands must have the same type, and it must be `int`/`bool` or `float`. The destination must match.

**Float-specific unary/binary (`modf`, `powf`, `negf`, `absf`, `sqrt`):**
All operands must be `float`.

**Bitwise (`and`, `or`, `xor`, `not`, `shl`, `shr`):**
All operands must be `int` or `bool`. Destination must be `int` or `bool`.

**Comparisons:**
Source operands must match each other. Destination must be `int` or `bool`.

**Casts:**
Each cast instruction requires a specific source type and produces a specific destination type (see [Instructions.md](Instructions.md)).

**Calls:**
Argument count and types are matched against the `.func` or `.extern` declaration. Return type of the callee must match the destination register's type. `bool` and `int` are considered matching. For variadic externs, at least the fixed parameters must match; extra arguments are unchecked.

**Function pointers:**
- `func.ptr dst, name` - `dst` must be `ptr`
- `call.ptr dst, fptr, args` - `fptr` must be `ptr`; `dst` receives the return value

**Load:**
The literal kind must be compatible with the destination type:
- integer literal -> `int` or `bool`
- float literal -> `float`
- char literal -> `char`
- `@name` data reference -> `ptr`

**Arrays:**
- `arr.new dst, size` - `dst` must be `ptr`
- `arr.len dst, arr` - `dst` must be `int` or `bool`
- `arr.get`, `arr.set` - no type restriction on elements (arrays are untyped 64-bit slots)

**Input:**
- `readchar dst` - `dst` must be `char`
- `readint dst` - `dst` must be `int` or `bool`
- `readfloat dst` - `dst` must be `float`
- `readline dst` - `dst` must be `ptr`

**Panic:**
- `panic reg` - `reg` must be `ptr` (points to a null-terminated error message string)

---

### Immediate literals

Literals can appear anywhere a source operand is expected - not just in `load`. The assembler automatically generates a load into a hidden scratch register before the instruction:

```asm
add r0, r0, 1
lt r1, r0, 100
shl r2, r0, 3
```

Destination operands must always be register names. Immediates are not allowed as destinations.

The type of an immediate is inferred from its form:
- digits only (optionally negative): `int`
- contains a decimal point: `float`
- single-quoted: `char`
- `@name`: `ptr`

Integer literals are compatible with both `int` and `bool` registers.

---

### Linter warnings

Beyond hard type errors, the linter emits warnings for:

- **Unused registers** - a local register that is declared but never read in any instruction.
- **Unreachable instructions** - instructions following an unconditional `jmp` or `ret` that can never be executed.

Warnings do not prevent assembly.

---

### Type mismatch examples

```asm
.func bad_arith() -> int
    int r0
    float r1
    load r0, 1
    load r1, 1.0
    add r0, r0, r1 ; error: type mismatch int vs float
    ret r0
.endfunc
```

```asm
.func bad_ret(int r0) -> float
    ret r0 ; error: ret type is int, function returns float
.endfunc
```

```asm
.func bad_bitwise() -> int
    float r0
    int r1
    load r0, 1.0
    load r1, 3
    and r1, r0, r1 ; error: bitwise operands must be int
    ret r1
.endfunc
```

```asm
; bool used as semantic annotation - interchangeable with int
.func is_even(int n) -> bool
    bool result
    int rem
    mod rem, n, 2
    eq result, rem, 0
    ret result
.endfunc
```

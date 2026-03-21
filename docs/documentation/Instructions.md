## Delta Assembly - Instructions

All instructions are written as `mnemonic operands` on a single line. Operands are separated by commas. The destination register (when present) is always first.

Notation used below:
- `dst` - destination register name
- `a`, `b` - source operands (register name or immediate)
- `reg` - any register name
- `label` - a label name defined in the same function
- `func` - a function name
- `ext` - an extern name

---

### Arithmetic

#### Integer

| Mnemonic | Syntax | Effect |
|---|---|---|
| `add` | `add dst, a, b` | `dst = a + b` |
| `sub` | `sub dst, a, b` | `dst = a - b` |
| `mul` | `mul dst, a, b` | `dst = a * b` |
| `div` | `div dst, a, b` | `dst = a / b` (truncating) |
| `mod` | `mod dst, a, b` | `dst = a % b` |
| `pow` | `pow dst, a, b` | `dst = aᵇ` (integer exponentiation) |
| `neg` | `neg dst, a` | `dst = -a` |
| `abs` | `abs dst, a` | `dst = \|a\|` |

All operands must be `int`. Overflow wraps. Division by zero is a runtime error.

#### Float

| Mnemonic | Syntax | Effect |
|---|---|---|
| `add` | `add dst, a, b` | `dst = a + b` |
| `sub` | `sub dst, a, b` | `dst = a - b` |
| `mul` | `mul dst, a, b` | `dst = a * b` |
| `div` | `div dst, a, b` | `dst = a / b` |
| `modf` | `modf dst, a, b` | `dst = fmod(a, b)` |
| `powf` | `powf dst, a, b` | `dst = aᵇ` |
| `negf` | `negf dst, a` | `dst = -a` |
| `absf` | `absf dst, a` | `dst = \|a\|` |
| `sqrt` | `sqrt dst, a` | `dst = √a` |

All operands must be `float`. The checker infers `add`/`sub`/`mul`/`div` type from the operand types, so the same mnemonic works for both `int` and `float`.

---

### Comparisons

All comparisons store `1` (true) or `0` (false) into an `int` or `bool` destination.

#### Integer

| Mnemonic | Meaning |
|---|---|
| `eq dst, a, b` | `a == b` |
| `ne dst, a, b` | `a != b` |
| `lt dst, a, b` | `a < b` |
| `le dst, a, b` | `a <= b` |
| `gt dst, a, b` | `a > b` |
| `ge dst, a, b` | `a >= b` |

#### Float

Same mnemonics - operands must both be `float`, `dst` must be `int` or `bool`.

#### Char

| Mnemonic | Meaning |
|---|---|
| `eq dst, a, b` | `a == b` |
| `ne dst, a, b` | `a != b` |

---

### Load

```
load dst, immediate
load dst, @data_name
```

Loads a constant into `dst`. The type of `dst` determines interpretation:

| Destination type | Immediate form |
|---|---|
| `int` or `bool` | integer literal: `42`, `-7`, `0` |
| `float` | float literal: `3.14`, `-0.5`, `1.0` |
| `char` | char literal: `'a'`, `'\n'`, `'\0'` |
| `ptr` | data reference: `@name` |

```
int r0
float r1
char r2
ptr r3

load r0, 100
load r1, 2.718
load r2, 'x'
load r3, @message
```

---

### Type casts

| Mnemonic | From -> To | Effect |
|---|---|---|
| `itof dst, src` | `int -> float` | convert integer to float |
| `ftoi dst, src` | `float -> int` | truncate float to integer |
| `itoc dst, src` | `int -> char` | reinterpret low 32 bits as Unicode scalar |
| `ctoi dst, src` | `char -> int` | zero-extend char to int |
| `ptoi dst, src` | `ptr -> int` | reinterpret pointer as integer |

---

### Control flow

```
jmp label
```
Unconditional jump to `label` in the same function.

```
jmpif    cond, label
jmpifnot cond, label
```
Jump if `cond` (an `int` or `bool` register) is non-zero / zero.

```
ret
ret operand
```
Return from the function. `ret` with no operand is used when the return type is `void`. Otherwise the operand must match the declared return type.

---

### Calls

```
call dst, func, arg, arg, ...
```
Call a `.func` and store the return value in `dst`.

```
call.void func, arg, arg, ...
```
Call a `.func` and discard the return value.

```
call.ext  dst, name, arg, arg, ...
call.ext.void  name, arg, arg, ...
```
Call an `.extern` function.

Arguments are operands (registers or immediates). The assembler checks argument count and types against the declaration.

---

### Memory

```
alloc dst, size
```
Allocate `size` bytes on the heap. `size` may be an `int` register or an integer immediate. Stores the resulting `ptr` in `dst`.

```
free reg
```
Free a heap-allocated pointer.

```
store ptr, val
```
Write `val` (any 64-bit register) into the memory pointed to by `ptr`.

```
read dst, ptr
```
Read 8 bytes from `ptr` into `dst`.

---

### Arrays

Arrays are heap-allocated contiguous blocks of 64-bit slots. Layout: `[length: i64][element₀: i64][element₁: i64]...`

| Mnemonic | Syntax | Effect |
|---|---|---|
| `arr.new` | `arr.new dst, size` | allocate array of `size` elements (`size` may be immediate or register) |
| `arr.get` | `arr.get dst, arr, idx` | `dst = arr[idx]` |
| `arr.set` | `arr.set arr, idx, val` | `arr[idx] = val` |
| `arr.len` | `arr.len dst, arr` | `dst = number of elements` |
| `arr.free` | `arr.free arr` | free the array |

`dst` of `arr.new` and `arr.len` must be `ptr` and `int` respectively. Elements are untyped - any 64-bit value can be stored.

---

### Bitwise operations

All operands must be `int` or `bool`. Destination must be `int` or `bool`.

| Mnemonic | Syntax | Effect |
|---|---|---|
| `and` | `and dst, a, b` | `dst = a & b` |
| `or` | `or dst, a, b` | `dst = a \| b` |
| `xor` | `xor dst, a, b` | `dst = a ^ b` |
| `not` | `not dst, a` | `dst = ~a` |
| `shl` | `shl dst, a, b` | `dst = a << b` (logical left shift) |
| `shr` | `shr dst, a, b` | `dst = a >> b` (arithmetic right shift) |

Shift amount is masked to the range `0-63`.

---

### Input (stdin)

| Mnemonic | Syntax | Effect |
|---|---|---|
| `readchar` | `readchar dst` | read one character; `dst` must be `char` |
| `readint` | `readint dst` | read a decimal integer; `dst` must be `int` or `bool` |
| `readfloat` | `readfloat dst` | read a float; `dst` must be `float` |
| `readline` | `readline dst` | read a line (newline stripped) into a heap-allocated string; `dst` must be `ptr` - caller must `free` |

---

### Immediate operands

Any source operand in an arithmetic, comparison, bitwise, or memory instruction may be a literal immediate rather than a register name. The assembler automatically generates a load into a hidden scratch register:

```asm
add r0, r0, 1
lt r1, r0, 100
shl r2, r0, 3
```

This applies to all binary and unary source operands. Destination operands must always be register names.

---

### Strings and chars

| Mnemonic | Syntax | Effect |
|---|---|---|
| `strlen` | `strlen dst, ptr` | `dst = length of null-terminated string` |
| `streq` | `streq dst, a, b` | `dst = 1` if strings equal, else `0` |
| `charat` | `charat dst, ptr, idx` | `dst = ptr[idx]` as `char` |
| `upper` | `upper dst, ch` | `dst = uppercase of ch` |
| `lower` | `lower dst, ch` | `dst = lowercase of ch` |
| `itos` | `itos dst, reg` | `dst = int-to-string (heap-allocated)` |
| `ftos` | `ftos dst, reg` | `dst = float-to-string (heap-allocated)` |

---

### Output

```
printint   reg   ; print int as decimal, no newline
printfloat reg   ; print float
printchar  reg   ; print single char
printptr   reg   ; print null-terminated string at pointer
```

`print` without a suffix selects the variant based on the register's declared type.

---

### Time

| Mnemonic | Stores in `dst` (`int`) |
|---|---|
| `timens dst` | Unix timestamp, nanoseconds |
| `timems dst` | Unix timestamp, milliseconds |
| `timemonons dst` | Monotonic nanoseconds since VM start |

---

### Function pointers

Function pointers allow passing functions as values and calling them indirectly. All delta functions have the same calling convention (all parameters and return value are 64-bit slots), so any function pointer can call any function with matching arity.

```
func.ptr dst, name
```
Load the address of `name` into `dst`. `dst` must be `ptr`.

```
call.ptr  dst, fptr, arg, arg, ...
call.ptr.void  fptr, arg, arg, ...
```
Call the function at address `fptr`. `call.ptr` stores the return value in `dst`; `call.ptr.void` discards it.

**Example - dispatch table:**
```asm
.section code

.func double(int r0) -> int
    int r1
    mul r1, r0, 2
    ret r1
.endfunc

.func triple(int r0) -> int
    int r1
    mul r1, r0, 3
    ret r1
.endfunc

; apply: calls whichever function pointer is passed
.func apply(ptr r0, int r1) -> int
    int r2
    call.ptr r2, r0, r1
    ret r2
.endfunc

.func main() -> int
    ptr fp
    int r

    func.ptr fp, double
    call r, apply, fp, 7
    printint r   ; 14

    func.ptr fp, triple
    call r, apply, fp, 7
    printint r   ; 21

    load r, 0
    ret r
.endfunc
```

---

### Variadic extern calls

Extern functions can be declared variadic with `...` at the end of the parameter list. The assembler allows any number of extra arguments beyond the fixed ones.

```
.extern printf(ptr, ...) -> int
```

```asm
.extern printf(ptr, ...) -> int

.section data
    .str fmt "%d plus %d = %d\n"

.section code
.func main() -> int
    ptr f
    int a
    int b
    int c
    load f, @fmt
    load a, 3
    load b, 4
    load c, 7
    call.ext.void printf, f, a, b, c   ; 3 plus 4 = 7
    load a, 0
    ret a
.endfunc
```

The interpreter handles `printf`, `malloc`, `free`, `exit`, `strlen`, and `strcmp` natively. Other variadic functions require the Cranelift backend (`--compile --emit exe`).

---

### Panic

```
panic reg
```

Prints `panic: <message>` to stdout and exits with code 1. `reg` must be a `ptr` to a null-terminated string.

```asm
.section data
    .str oob "index out of bounds"

.section code
.func checked_get(ptr r0, int r1) -> int
    int r2
    ptr msg
    arr.len r2, r0
    lt r2, r1, r2
    jmpif r2, ok
    load msg, @oob
    panic msg
ok
    arr.get r2, r0, r1
    ret r2
.endfunc
```

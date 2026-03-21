## Delta Assembly - File Structure

A `.ds` file is plain UTF-8 text. Lines are significant: most constructs are one per line. Comments start with `;` and run to end of line. Blank lines and extra whitespace between tokens are ignored.

---

### Top-level layout

```
; optional extern declarations
.extern putchar(char) -> int
.extern malloc(int) -> ptr

; optional data section
.section data
    .str greeting "Hello, world!\n"
    .i64 answer 42

; optional code section marker (may be omitted)
.section code

.func main() -> int
    ; ...
.endfunc
```

Top-level items can appear in any order. The assembler does two passes so forward references to functions and data names always work.

---

### Extern declarations

```
.extern name(type, type, ...) -> return_type
.extern name(type, type, ..., ...) -> return_type   ; variadic
```

Declares a C function to be called at runtime. The parameter list contains only types, no names. Return type may be `void`. Add `...` as the last parameter to declare a variadic function (like `printf`).

```
.extern printf(ptr, ...) -> int
.extern free(ptr) -> void
.extern memcpy(ptr, ptr, int) -> ptr
```

Extern functions are called with `call.ext` / `call.ext.void`. The assembler verifies argument count and types at the call site.

---

### Data section

```
.section data
    .str name "string value"
    .i64 name integer_value
```

`.str` stores a null-terminated UTF-8 string. Escape sequences: `\n \t \r \0 \\ \"`.

`.i64` stores a 64-bit signed integer constant.

Data items are referenced in code with `@name`:

```
load r0, @greeting   ; r0 : ptr  ->  address of the string
```

There is no `.f64` data directive - float immediates are written inline in code as literals.

---

### Functions

```
.func name(type param, type param, ...) -> return_type
    type register
    type register
    ...
    instruction
    instruction
    ...
.endfunc
```

**Parameters** are declared in the signature. They are the first registers of the function and are pre-loaded with the caller's arguments.

**Local registers** are declared at the top of the body, before any instructions. A register declaration is just a type followed by a name:

```
int r0
float r1
ptr buf
char ch
bool flag
```

All registers must be declared before use. The assembler rejects references to undeclared names.

**Labels** are bare identifiers on their own line, with no colon:

```
loop
    add r0, r0, r1
    lt r2, r0, r3
    jmpif r2, loop
```

Labels are local to their function. Two functions may use the same label name.

---

### Types

| Type | Size | Holds |
|---|---|---|
| `int` | 64-bit | signed integer |
| `bool` | 64-bit | boolean (0/1), alias for `int` |
| `float` | 64-bit | IEEE 754 double |
| `char` | 32-bit | Unicode scalar value |
| `ptr` | 64-bit | heap pointer or data-section address |
| `void` | - | return type only; no register may have this type |

All registers are stored as 64-bit slots internally. The type is a compile-time annotation only.

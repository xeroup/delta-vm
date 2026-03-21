## Delta Assembly - Examples

---

### Hello, world

```asm
.section data
    .str msg "Hello, world!\n"

.section code

.func main() -> int
    ptr r0
    int r1
    load r0, @msg
    printptr r0
    load r1, 0
    ret r1
.endfunc
```

---

### Fibonacci (recursive)

```asm
.section code

.func fib(int r0) -> int
    int r1
    int r2
    lt r2, r0, 2
    jmpif r2, base
    sub r1, r0, 1
    call r1, fib, r1
    sub r2, r0, 2
    call r2, fib, r2
    add r1, r1, r2
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
    ret r1
.endfunc
```

---

### Loops and immediates

Source operands can be literals directly:

```asm
.section code

.func sum_to(int r0) -> int
    int r1
    int r2
    int r3
    load r1, 0
    load r2, 1
loop
    gt r3, r2, r0
    jmpif r3, done
    add r1, r1, r2
    add r2, r2, 1
    jmp loop
done
    ret r1
.endfunc

.func main() -> int
    int r0
    int r1
    load r0, 100
    call r1, sum_to, r0
    printint r1 ; 5050
    ret r1
.endfunc
```

---

### Function pointers

Functions can be stored in `ptr` registers and called indirectly:

```asm
.section code

.func double(int r0) -> int
    int r1
    add r1, r0, r0
    ret r1
.endfunc

.func triple(int r0) -> int
    int r1
    mul r1, r0, 3
    ret r1
.endfunc

; higher-order: apply a function pointer to a value
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
    printint r ; 14

    func.ptr fp, triple
    call r, apply, fp, 7
    printint r ; 21

    ; direct indirect call
    func.ptr fp, double
    call.ptr r, fp, 5
    printint r ; 10

    load r, 0
    ret r
.endfunc
```

---

### Variadic extern (printf)

```asm
.extern printf(ptr, ...) -> int

.section data
    .str fmt "%s has %d items\n"

.section code
.func main() -> int
    ptr f
    ptr s
    int n
    int r

    load f, @fmt
    load s, @fmt
    load n, 42

    call.ext.void printf, f, s, n

    load r, 0
    ret r
.endfunc
```

```sh
$ dvm program.ds --entry main
 has 42 items
```

A more complete example with multiple format specifiers:

```asm
.extern printf(ptr, ...) -> int

.section data
    .str fmt "%d + %d = %d\n"

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
    call.ext.void printf, f, a, b, c ; 3 + 4 = 7
    ret a
.endfunc
```

---

### Panic and bounds checking

```asm
.section data
    .str err_oob "array index out of bounds"

.section code

.func safe_get(ptr r0, int r1) -> int
    int r2
    ptr msg
    arr.len r2, r0
    lt r2, r1, r2
    jmpif r2, ok
    load msg, @err_oob
    panic msg
ok
    arr.get r2, r0, r1
    ret r2
.endfunc

.func main() -> int
    ptr arr
    int v
    arr.new arr, 5
    load v, 0
    arr.set arr, 0, v
    load v, 99
    arr.set arr, 4, v

    call v, safe_get, arr, 4
    printint v ; 99

    arr.free arr
    ret v
.endfunc
```

---

### Arrays

```asm
.section code

.func main() -> int
    ptr arr
    int i
    int v
    int len

    arr.new arr, 8
    arr.len len, arr

    load i, 0
fill
    lt v, i, len
    jmpifnot v, done
    mul v, i, i
    arr.set arr, i, v
    add i, i, 1
    jmp fill

done
    load i, 5
    arr.get v, arr, i
    printint v ; 25
    arr.free arr
    load i, 0
    ret i
.endfunc
```

---

### Bitwise operations

```asm
.section code

.func main() -> int
    int flags
    int mask
    int r

    load flags, 0
    or flags, flags, 8 ; set bit 3
    and r, flags, 8
    printint r ; 8

    xor flags, flags, 8 ; clear bit 3
    and r, flags, 8
    printint r ; 0

    load r, 7
    shl r, r, 2
    printint r ; 28

    ret r
.endfunc
```

---

### Input from stdin

```asm
.section code

.func main() -> int
    int n
    ptr line

    readint n
    readline line

    mul n, n, 2
    printint n
    printptr line
    free line

    ret n
.endfunc
```

```sh
$ printf "21\nhello\n" | dvm input.ds --entry main
42hello
```

---

### Native compilation

```sh
# interpret
dvm program.ds --entry main

# compile to native executable (no external linker needed)
dvm program.ds --entry main --compile --emit exe -o program
./program

# other emit targets
dvm program.ds --entry main --compile --emit asm
dvm program.ds --entry main --compile --emit obj
```

Native binaries typically run 10-25x faster than the interpreter for compute-intensive code.

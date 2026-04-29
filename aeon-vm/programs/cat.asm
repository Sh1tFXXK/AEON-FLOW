; cat.asm
; Demonstrate the VFS syscall workflow.
;
; Creates "cat.txt" with bytes "cat", reopens it for reading, then reads and
; prints each byte value with the VM print instruction.
;
; Syscalls:
;   0 = open  (r1=path_addr, r2=path_len, r3=mode; r0=fd)
;   1 = read  (r1=fd, r2=buf_addr, r3=count; r0=bytes_read)
;   2 = write (r1=fd, r2=buf_addr, r3=count; r0=bytes_written)
;   3 = close (r1=fd; r0=0)

    load r9, 1

; path "cat.txt" at heap[0..7]
    load r7, 0
    load r8, 99
    storemem r7, r8
    add r7, r7, r9
    load r8, 97
    storemem r7, r8
    add r7, r7, r9
    load r8, 116
    storemem r7, r8
    add r7, r7, r9
    load r8, 46
    storemem r7, r8
    add r7, r7, r9
    load r8, 116
    storemem r7, r8
    add r7, r7, r9
    load r8, 120
    storemem r7, r8
    add r7, r7, r9
    load r8, 116
    storemem r7, r8

; contents "cat" at heap[32..35]
    load r7, 32
    load r8, 99
    storemem r7, r8
    add r7, r7, r9
    load r8, 97
    storemem r7, r8
    add r7, r7, r9
    load r8, 116
    storemem r7, r8

; fd = open("cat.txt", write)
    load r1, 0
    load r2, 7
    load r3, 1
    syscall 0
    mov r4, r0

; write(fd, heap[32..35])
    mov r1, r4
    load r2, 32
    load r3, 3
    syscall 2

; close(fd)
    mov r1, r4
    syscall 3

; fd = open("cat.txt", read)
    load r1, 0
    load r2, 7
    load r3, 0
    syscall 0
    mov r4, r0

; Read and print 3 bytes, one at a time.
    load r5, 3
read_loop:
    jz r5, done
    mov r1, r4
    load r2, 64
    load r3, 1
    syscall 1
    load r7, 64
    loadmem r6, r7
    print r6
    sub r5, r5, r9
    jmp read_loop

done:
    mov r1, r4
    syscall 3
    halt

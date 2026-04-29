; bubble_sort.asm
; Sort five one-byte values in heap using bubble sort.
;
; Initial heap[0..5): [5, 2, 8, 1, 9]
; At snap-at 30:       [5, 2, 1, 1, 9]  ; first swap half-written
; Expected heap[0..5): [1, 2, 5, 8, 9]
;
; Registers:
;   r10 = base address
;   r11..r14 = constants 1..4
;   r20/r21 = compare_swap address inputs
;   r22/r23 = compare_swap values
;   r24/r25 = compare counters

    load r0, 5
    alloc r10, r0
    load r11, 1

    load r3, 5
    storemem r10, r3
    add r2, r10, r11
    load r3, 2
    storemem r2, r3
    add r2, r2, r11
    mov r20, r2
    load r3, 8
    storemem r2, r3
    add r2, r2, r11
    mov r21, r2
    load r3, 1
    storemem r2, r3
    add r2, r2, r11
    load r3, 9
    storemem r2, r3

; Inline compare-swap heap[2] and heap[3] so snap-at 30 shows a mid-swap heap.
    loadmem r22, r20
    loadmem r23, r21
    mov r24, r22
    mov r25, r23
first_compare_loop:
    jz r25, first_do_swap
    jz r24, first_compare_done
    sub r24, r24, r11
    sub r25, r25, r11
    jmp first_compare_loop
first_do_swap:
    storemem r20, r23
    storemem r21, r22
first_compare_done:

    load r12, 2
    load r13, 3
    load r14, 4

; pass 1
    mov r20, r10
    add r21, r10, r11
    call compare_swap
    add r20, r10, r11
    add r21, r10, r12
    call compare_swap
    add r20, r10, r12
    add r21, r10, r13
    call compare_swap
    add r20, r10, r13
    add r21, r10, r14
    call compare_swap

; pass 2
    mov r20, r10
    add r21, r10, r11
    call compare_swap
    add r20, r10, r11
    add r21, r10, r12
    call compare_swap
    add r20, r10, r12
    add r21, r10, r13
    call compare_swap

; pass 3
    mov r20, r10
    add r21, r10, r11
    call compare_swap
    add r20, r10, r11
    add r21, r10, r12
    call compare_swap

; pass 4
    mov r20, r10
    add r21, r10, r11
    call compare_swap

; mirror sorted heap values into registers for non-interactive runs
    mov r2, r10
    loadmem r30, r2
    add r2, r10, r11
    loadmem r31, r2
    add r2, r10, r12
    loadmem r32, r2
    add r2, r10, r13
    loadmem r33, r2
    add r2, r10, r14
    loadmem r34, r2
    halt

compare_swap:
    loadmem r22, r20
    loadmem r23, r21
    mov r24, r22
    mov r25, r23
compare_loop:
    jz r25, do_swap
    jz r24, compare_done
    sub r24, r24, r11
    sub r25, r25, r11
    jmp compare_loop
do_swap:
    storemem r20, r23
    storemem r21, r22
compare_done:
    ret

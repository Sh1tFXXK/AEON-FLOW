; fibonacci.asm
; Compute the nth Fibonacci number iteratively.
;
; Input:  r0 = n  (set via LoadImm below)
; Output: r2 = fib(n)
;
; Register layout:
;   r0 = countdown
;   r1 = a (previous)
;   r2 = b (current / result)
;   r3 = temp
;   r4 = 1 (constant)

    load r0, 10     ; compute fib(10)
    load r1, 0      ; a = 0
    load r2, 1      ; b = 1
    load r4, 1      ; constant 1

loop:
    add  r3, r1, r2 ; temp = a + b
    mov  r1, r2     ; a = b
    mov  r2, r3     ; b = temp
    sub  r0, r0, r4 ; countdown -= 1
    jz   r0, end    ; if done: jump to end
    jmp  loop       ; else: repeat

end:
    halt

; Expected result: r2 = 55

; factorial.asm
; Compute n! iteratively.
;
; Input:  r0 = n
; Output: r1 = n!
;
; Register layout:
;   r0 = countdown
;   r1 = accumulator
;   r2 = 1 (constant)

    load r0, 7      ; n = 7
    load r1, 1      ; acc = 1
    load r2, 1      ; constant 1

loop:
    mul  r1, r1, r0 ; acc *= n
    sub  r0, r0, r2 ; n -= 1
    jz   r0, end    ; if n==0: done
    jmp  loop

end:
    halt

; Expected result: r1 = 5040

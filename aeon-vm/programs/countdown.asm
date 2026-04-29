; countdown.asm
; Count r0 down from 10 to 0.
;
; Step trace:
;   after step 1:  r0 = 10
;   after step 2:  r1 = 1
;   after step 4:  r0 = 9
;   after step 7:  r0 = 8
;   after step 10: r0 = 7
;   after step 13: r0 = 6
;   after step 16: r0 = 5
;   after step 19: r0 = 4
;   after step 22: r0 = 3
;   after step 25: r0 = 2
;   after step 28: r0 = 1
;   after step 31: r0 = 0

    load r0, 10
    load r1, 1
loop:
    jz r0, done
    sub r0, r0, r1
    jmp loop
done:
    halt

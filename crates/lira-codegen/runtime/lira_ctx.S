/*
 * Fiber context switch.
 *
 *   void lira_ctx_switch(void **save_sp, void *new_sp);
 *
 * Pushes the callee-saved registers of the ABI onto the current stack, records
 * the resulting stack pointer through `save_sp`, then restores the mirror-image
 * frame from `new_sp` and returns onto the other fiber's stack. Caller-saved
 * registers need no attention: the C compiler already treats a call to this
 * function as clobbering them.
 *
 * `lira_fiber_trampoline` is the synthetic return address a freshly created
 * fiber's frame carries. It moves the fiber pointer (parked in a callee-saved
 * register by lira_fiber_init_stack) into the first argument register and calls
 * into C, which never comes back.
 */

#if defined(__APPLE__)
#define LIRA_SYM(name) _##name
#else
#define LIRA_SYM(name) name
#endif

#if defined(__ELF__)
#define LIRA_TYPE(name) .type LIRA_SYM(name), %function
#define LIRA_SIZE(name) .size LIRA_SYM(name), . - LIRA_SYM(name)
#else
#define LIRA_TYPE(name)
#define LIRA_SIZE(name)
#endif

#define LIRA_FUNC(name)      \
    .globl LIRA_SYM(name);   \
    LIRA_TYPE(name);         \
    .p2align 4;              \
    LIRA_SYM(name):

/* ==================================================================== */
#if defined(__x86_64__)
/* ==================================================================== */

    .text

LIRA_FUNC(lira_ctx_switch)
    pushq %rbp
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    movq  %rsp, (%rdi)
    movq  %rsi, %rsp
    popq  %r15
    popq  %r14
    popq  %r13
    popq  %r12
    popq  %rbx
    popq  %rbp
    ret
    LIRA_SIZE(lira_ctx_switch)

LIRA_FUNC(lira_fiber_trampoline)
    /* r15 holds the LiraFiber* planted by lira_fiber_init_stack. RSP is
     * 16-byte aligned here, which is exactly what the call below wants. */
    movq %r15, %rdi
    xorq %rbp, %rbp
    call LIRA_SYM(lira_fiber_start)
    ud2
    LIRA_SIZE(lira_fiber_trampoline)

/* ==================================================================== */
#elif defined(__aarch64__)
/* ==================================================================== */

    .text

LIRA_FUNC(lira_ctx_switch)
    sub  sp, sp, #160
    stp  x19, x20, [sp, #0]
    stp  x21, x22, [sp, #16]
    stp  x23, x24, [sp, #32]
    stp  x25, x26, [sp, #48]
    stp  x27, x28, [sp, #64]
    stp  x29, x30, [sp, #80]
    stp  d8,  d9,  [sp, #96]
    stp  d10, d11, [sp, #112]
    stp  d12, d13, [sp, #128]
    stp  d14, d15, [sp, #144]
    mov  x2, sp
    str  x2, [x0]
    mov  sp, x1
    ldp  x19, x20, [sp, #0]
    ldp  x21, x22, [sp, #16]
    ldp  x23, x24, [sp, #32]
    ldp  x25, x26, [sp, #48]
    ldp  x27, x28, [sp, #64]
    ldp  x29, x30, [sp, #80]
    ldp  d8,  d9,  [sp, #96]
    ldp  d10, d11, [sp, #112]
    ldp  d12, d13, [sp, #128]
    ldp  d14, d15, [sp, #144]
    add  sp, sp, #160
    ret
    LIRA_SIZE(lira_ctx_switch)

LIRA_FUNC(lira_fiber_trampoline)
    /* x19 holds the LiraFiber* planted by lira_fiber_init_stack. */
    mov  x0, x19
    mov  x29, #0
    mov  x30, #0
    bl   LIRA_SYM(lira_fiber_start)
    brk  #1
    LIRA_SIZE(lira_fiber_trampoline)

/* ==================================================================== */
#else
/* ==================================================================== */
#error "the Lira native backend supports x86_64 and aarch64 only"
#endif

#if defined(__ELF__)
/* Nothing here needs an executable stack. */
.section .note.GNU-stack,"",%progbits
#endif

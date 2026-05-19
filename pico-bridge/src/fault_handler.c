// HardFault / BusFault / UsageFault / MemManage handler.
//
// Captures the stacked exception frame (PC, LR, xPSR), persists it
// via reset_reason_record_fault, then triggers an explicit system
// reset via SCB->AIRCR. The watchdog is not used to force the reset:
// on Cortex-M0+ the watchdog tick IRQ has lower priority than
// HardFault, so it cannot preempt the handler. SCB AIRCR works on
// both Cortex-M0+ (RP2040) and Cortex-M33 (RP2350).
//
// Why these handlers and not the Pico SDK defaults: the SDK weak
// handlers spin forever in `__breakpoint`, which on a release build
// is invisible to the host -- the Pico simply stops responding,
// USB drops, and the operator sees "the Pico hung" with no clue
// what state it was in. With this handler the cause is in the next
// boot's diag_log.

#include <stdint.h>

#include "reset_reason.h"

// SCB AIRCR address: same on all ARMv6-M / ARMv7-M / ARMv8-M cores.
#define SCB_AIRCR_ADDR 0xE000ED0Cu
#define SCB_AIRCR_VECTKEY_SYSRESETREQ 0x05FA0004u

// Trampoline target. The naked handlers below set r0 = stack pointer
// of the interrupted code's frame, then branch here. Layout of the
// basic exception frame (offsets in 32-bit words):
//   [0] r0
//   [1] r1
//   [2] r2
//   [3] r3
//   [4] r12
//   [5] LR (return address from the interrupted function)
//   [6] PC (where the fault occurred)
//   [7] xPSR
__attribute__((used))
static void fault_handler_c(uint32_t *frame) {
    // Record the entire stacked basic frame plus the stack pointer
    // value at the time of the fault. reset_reason_record_fault peeks
    // at the Cortex-M33 SCB->CFSR/HFSR/MMFAR/BFAR internally when the
    // build targets RP2350.
    reset_reason_record_fault(frame, (uint32_t)frame);
    // Trigger SCB system-reset. The compiler may not have a barrier
    // around the volatile write, so an explicit dsb is added.
    volatile uint32_t *aircr = (volatile uint32_t *)SCB_AIRCR_ADDR;
    *aircr = SCB_AIRCR_VECTKEY_SYSRESETREQ;
    __asm volatile ("dsb" ::: "memory");
    for (;;) { /* unreachable */ }
}

// Naked handler: figure out which stack the frame is on (MSP vs PSP),
// pass it to fault_handler_c. The Pico SDK does not use PSP in
// foreground code, but the EXC_RETURN check is cheap and means this
// handler also works if a future RTOS port shows up.
__attribute__((naked))
void isr_hardfault(void) {
    __asm volatile (
        "movs r0, #4\n"
        "mov  r1, lr\n"
        "tst  r0, r1\n"
        "beq  1f\n"
        "mrs  r0, psp\n"
        "b    2f\n"
        "1:\n"
        "mrs  r0, msp\n"
        "2:\n"
        "ldr  r1, =fault_handler_c\n"
        "bx   r1\n"
    );
}

// RP2350 (Cortex-M33) splits faults into multiple vectors. Override
// each so a more specific fault still lands in our capture path
// rather than the SDK's silent spin. On RP2040 (Cortex-M0+) these
// vectors do not exist; the SDK conditionally omits the symbols
// based on PICO_RP2040, so these extra definitions are gated to
// match.
#if !PICO_RP2040

__attribute__((naked))
void isr_busfault(void) {
    __asm volatile (
        "movs r0, #4\n"
        "mov  r1, lr\n"
        "tst  r0, r1\n"
        "beq  1f\n"
        "mrs  r0, psp\n"
        "b    2f\n"
        "1:\n"
        "mrs  r0, msp\n"
        "2:\n"
        "ldr  r1, =fault_handler_c\n"
        "bx   r1\n"
    );
}

__attribute__((naked))
void isr_usagefault(void) {
    __asm volatile (
        "movs r0, #4\n"
        "mov  r1, lr\n"
        "tst  r0, r1\n"
        "beq  1f\n"
        "mrs  r0, psp\n"
        "b    2f\n"
        "1:\n"
        "mrs  r0, msp\n"
        "2:\n"
        "ldr  r1, =fault_handler_c\n"
        "bx   r1\n"
    );
}

__attribute__((naked))
void isr_memmanage(void) {
    __asm volatile (
        "movs r0, #4\n"
        "mov  r1, lr\n"
        "tst  r0, r1\n"
        "beq  1f\n"
        "mrs  r0, psp\n"
        "b    2f\n"
        "1:\n"
        "mrs  r0, msp\n"
        "2:\n"
        "ldr  r1, =fault_handler_c\n"
        "bx   r1\n"
    );
}

#endif  // !PICO_RP2040

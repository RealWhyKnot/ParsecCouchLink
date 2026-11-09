#include <stdint.h>

#include "reset_reason.h"

#define SCB_AIRCR_ADDR 0xE000ED0Cu
#define SCB_AIRCR_VECTKEY_SYSRESETREQ 0x05FA0004u

__attribute__((used)) static void fault_handler_c(uint32_t *frame) {
    reset_reason_record_fault(frame, (uint32_t)frame);
    volatile uint32_t *aircr = (volatile uint32_t *)SCB_AIRCR_ADDR;
    *aircr = SCB_AIRCR_VECTKEY_SYSRESETREQ;
    __asm volatile("dsb" ::: "memory");
    for (;;) {
    }
}

__attribute__((naked)) void isr_hardfault(void) {
    __asm volatile("movs r0, #4\n"
                   "mov  r1, lr\n"
                   "tst  r0, r1\n"
                   "beq  1f\n"
                   "mrs  r0, psp\n"
                   "b    2f\n"
                   "1:\n"
                   "mrs  r0, msp\n"
                   "2:\n"
                   "ldr  r1, =fault_handler_c\n"
                   "bx   r1\n");
}

#if !PICO_RP2040

__attribute__((naked)) void isr_busfault(void) {
    __asm volatile("movs r0, #4\n"
                   "mov  r1, lr\n"
                   "tst  r0, r1\n"
                   "beq  1f\n"
                   "mrs  r0, psp\n"
                   "b    2f\n"
                   "1:\n"
                   "mrs  r0, msp\n"
                   "2:\n"
                   "ldr  r1, =fault_handler_c\n"
                   "bx   r1\n");
}

__attribute__((naked)) void isr_usagefault(void) {
    __asm volatile("movs r0, #4\n"
                   "mov  r1, lr\n"
                   "tst  r0, r1\n"
                   "beq  1f\n"
                   "mrs  r0, psp\n"
                   "b    2f\n"
                   "1:\n"
                   "mrs  r0, msp\n"
                   "2:\n"
                   "ldr  r1, =fault_handler_c\n"
                   "bx   r1\n");
}

__attribute__((naked)) void isr_memmanage(void) {
    __asm volatile("movs r0, #4\n"
                   "mov  r1, lr\n"
                   "tst  r0, r1\n"
                   "beq  1f\n"
                   "mrs  r0, psp\n"
                   "b    2f\n"
                   "1:\n"
                   "mrs  r0, msp\n"
                   "2:\n"
                   "ldr  r1, =fault_handler_c\n"
                   "bx   r1\n");
}

#endif

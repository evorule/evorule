/* mps2-an386 (QEMU Cortex-M4 emulation platform) memory layout
 * FLASH: ZBT SSRAM1 @ 0x00000000 (4 MiB) — vector table + code
 * RAM:   SRAM @ 0x20000000 (4 MiB) — data / heap
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 4M
  RAM   : ORIGIN = 0x20000000, LENGTH = 4M
}

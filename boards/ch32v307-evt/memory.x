/* CH32V307VCT6 memory layout (256KB Flash / 64KB RAM, riscv-rt 0.18 REGION_*
 * convention)
 *
 * The last 8 Flash pages, 8 x 4KB = 32KB (0x08038000..0x08040000), form the
 * WAL tail-page ring, intentionally without a REGION — that area is accessed
 * only at fixed addresses by board code (ring layout starts at 8 pages; when
 * parameterizing a larger capacity, adjust REGION_TEXT/RODATA lengths and the
 * notes in this file in lockstep).
 *
 * RAM balancing note: the TLSF static pool (8192 words = 32KB + header
 * overhead) lives in .bss; REGION_BSS must be >= 36KB. REGION_HEAP is a
 * placeholder only (the actual heap is the pool in .bss).
 */
MEMORY
{
  REGION_TEXT   : ORIGIN = 0x08000000, LENGTH = 192K
  REGION_RODATA : ORIGIN = 0x08030000, LENGTH = 32K
  REGION_DATA   : ORIGIN = 0x20000000, LENGTH = 8K
  REGION_BSS    : ORIGIN = 0x20002000, LENGTH = 40K
  REGION_HEAP   : ORIGIN = 0x2000C000, LENGTH = 4K
  REGION_STACK  : ORIGIN = 0x2000D000, LENGTH = 12K
}

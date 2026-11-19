/* STM32F407VGT6 memory layout (1MB Flash / 192KB RAM)
 *
 * Flash WAL region = sector0-3 (0x08000000..0x08010000, 4x16KB = 64KB),
 * intentionally without a REGION — code starts at sector4; the WAL region is
 * accessed only at fixed addresses by board code.
 */
MEMORY
{
  FLASH : ORIGIN = 0x08010000, LENGTH = 960K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}

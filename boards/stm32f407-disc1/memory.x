/* STM32F407VGT6 内存布局（1MB Flash / 192KB RAM）
 *
 * Flash WAL 区段 = sector0-3（0x08000000..0x08010000，4×16KB=64KB），
 * 有意不设 REGION——代码从 sector4 起，WAL 区仅由板级代码固定地址访问。
 */
MEMORY
{
  FLASH : ORIGIN = 0x08010000, LENGTH = 960K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}

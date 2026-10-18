/* mps2-an386（QEMU Cortex-M4 仿真平台）存储布局
 * FLASH: ZBT SSRAM1 @ 0x00000000（4 MiB）——向量表 + 代码
 * RAM:   SRAM @ 0x20000000（4 MiB）——数据 / 堆
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 4M
  RAM   : ORIGIN = 0x20000000, LENGTH = 4M
}

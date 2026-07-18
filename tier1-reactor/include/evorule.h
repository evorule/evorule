/**
 * @file evorule.h
 * @brief evorule C FFI 接口定义
 *
 * TheEquation 反应式执行器 - 事实驱动的状态转换引擎
 *
 * # 使用示例
 * ```c
 * #include <evorule.h>
 *
 * int main() {
 *     evorule_reactor* reactor = evorule_reactor_new();
 *
 *     // 发送命令
 *     evorule_reactor_send_command(reactor, "{\"type\": \"increment\"}");
 *
 *     // 调试控制
 *     evorule_reactor_pause(reactor);
 *     evorule_reactor_step(reactor, 5);
 *     evorule_reactor_resume(reactor);
 *
 *     // 查询状态
 *     int queue_size = evorule_reactor_current_queue_size(reactor);
 *     int is_paused = evorule_reactor_is_paused(reactor);
 *
 *     evorule_reactor_free(reactor);
 *     return 0;
 * }
 * ```
 */

#ifndef EVORULE_H
#define EVORULE_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>

/**
 * @brief 错误码枚举
 */
typedef enum evorule_error_code {
    EVORULE_OK = 0,
    EVORULE_ERROR_OOM = 1,
    EVORULE_ERROR_INVALID_ARG = 2,
    EVORULE_ERROR_RUNTIME = 3,
    EVORULE_ERROR_NOT_INITIALIZED = 4,
} evorule_error_code_t;

/**
 * @brief 反应器句柄（opaque pointer）
 */
typedef void evorule_reactor;

/**
 * @brief 结果句柄（opaque pointer）
 */
typedef void evorule_result;

/**
 * @brief 获取库版本号
 * @return 版本字符串指针，不需要调用者释放
 */
const char* evorule_version(void);

/**
 * @brief 释放由 evorule 返回的字符串
 * @param s 字符串指针
 */
void evorule_free_string(char* s);

/**
 * @brief 创建新的反应器实例
 * @return 反应器句柄，失败返回 NULL
 */
evorule_reactor* evorule_reactor_new(void);

/**
 * @brief 销毁反应器实例
 * @param reactor 反应器句柄
 */
void evorule_reactor_free(evorule_reactor* reactor);

/**
 * @brief 发送命令到反应器
 * @param reactor 反应器句柄
 * @param instruction_json JSON 格式的指令字符串
 * @return 错误码
 */
evorule_error_code_t evorule_reactor_send_command(
    evorule_reactor* reactor,
    const char* instruction_json
);

/**
 * @brief 暂停反应器执行
 * @param reactor 反应器句柄
 * @return 错误码
 */
evorule_error_code_t evorule_reactor_pause(evorule_reactor* reactor);

/**
 * @brief 恢复反应器执行
 * @param reactor 反应器句柄
 * @return 错误码
 */
evorule_error_code_t evorule_reactor_resume(evorule_reactor* reactor);

/**
 * @brief 执行指定步数后暂停
 * @param reactor 反应器句柄
 * @param n 步数
 * @return 结果句柄，失败返回 NULL
 */
evorule_result* evorule_reactor_step(evorule_reactor* reactor, int n);

/**
 * @brief 获取当前队列大小
 * @param reactor 反应器句柄
 * @return 队列大小，失败返回 -1
 */
int evorule_reactor_current_queue_size(evorule_reactor* reactor);

/**
 * @brief 查询反应器是否暂停
 * @param reactor 反应器句柄
 * @return 1=暂停，0=运行中，-1=错误
 */
int evorule_reactor_is_paused(evorule_reactor* reactor);

/**
 * @brief 获取结果输出
 * @param result 结果句柄
 * @return 输出字符串指针，调用者需要调用 evorule_free_string 释放
 */
const char* evorule_result_get_output(evorule_result* result);

/**
 * @brief 释放结果句柄
 * @param result 结果句柄
 */
void evorule_result_free(evorule_result* result);

#ifdef __cplusplus
}
#endif

#endif /* EVORULE_H */
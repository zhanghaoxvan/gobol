#ifndef VARIADIC_PRINTF_H
#define VARIADIC_PRINTF_H

// 解决 MSVC 的“安全检查”警告（否则 C4996 会被 /WX 转化为致命错误）
#ifdef _MSC_VER
#pragma warning(disable: 4996)
#endif

// 函数声明：跨平台的 printf，标准 C 变参形式
int printf(const char *format, ...);

#endif // VARIADIC_PRINTF_H

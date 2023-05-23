/*
  This file is a modified version of the original "logger.cpp" in mikanos.
*/
#include "logger.hpp"

#include <cstddef>
#include <cstdio>

// #include "console.hpp"

namespace {
  LogLevel log_level = kWarn;
}

// extern Console* console;

void SetLogLevel(LogLevel level) {
  log_level = level;
}

void (*print_fn)(const char* s) = NULL;

void SetPrintFn(void (*fn)(const char* s)) {
  print_fn = fn;
}

int Log(LogLevel level, const char* format, ...) {
  if (level > log_level) {
    return 0;
  }

  va_list ap;
  int result;
  char s[1024];

  va_start(ap, format);
  result = vsprintf(s, format, ap);
  va_end(ap);

  if (print_fn) print_fn(s);

  return result;
}

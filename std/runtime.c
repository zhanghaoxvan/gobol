/*
 * runtime.c — Gobol AOT runtime master translation unit.
 *
 * This file is compiled as a single translation unit that pulls in all
 * sub-modules.  The compiler (gobol.rs) links this file when producing
 * standalone executables with the Cranelift ObjectModule backend.
 *
 * See runtime/ for individual module sources.
 */
#include "runtime/platform.h"   /* must come first — platform abstractions */
#include "runtime/types.h"
#include "runtime/io.c"
#include "runtime/str.c"
#include "runtime/mem.c"
#include "runtime/array.c"
#include "runtime/math.c"
#include "runtime/fs.c"
#include "runtime/net.c"
#include "runtime/thread.c"
#include "runtime/channel.c"
#include "runtime/gc.c"
#include "runtime/entry.c"

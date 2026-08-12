/*
 * runtime.h — Gobol AOT runtime master header.
 *
 * Includes all sub-headers under runtime/ so users of the runtime only
 * need a single #include.
 */
#ifndef GOBOL_RUNTIME_H
#define GOBOL_RUNTIME_H

#include "runtime/io.h"
#include "runtime/str.h"
#include "runtime/mem.h"
#include "runtime/array.h"
#include "runtime/math.h"
#include "runtime/fs.h"
#include "runtime/net.h"
#include "runtime/thread.h"
#include "runtime/channel.h"
#include "runtime/gc.h"
#include "runtime/entry.h"

#endif /* GOBOL_RUNTIME_H */

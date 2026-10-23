#ifndef BBCORE_H
#define BBCORE_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

// The outcome of a call. Only `BB_OK` carries a result.
typedef enum BbStatus {
  // The call succeeded.
  BB_OK = 0,
  // The call failed; `bb_last_error` has the message.
  BB_ERR = 1,
  // Rust panicked inside the call; `bb_last_error` has the panic message.
  BB_PANIC = 2,
  // A handle or id named something that no longer exists.
  BB_STALE = 3,
} BbStatus;

// A borrowed UTF-16 string view. Not NUL-terminated.
typedef struct BbStr {
  const uint16_t *ptr;
  uint32_t len;
} BbStr;

// A UTF-16 string the caller owns. Free it with `bb_str_free`.
typedef struct BbOwnedStr {
  uint16_t *ptr;
  uint32_t len;
} BbOwnedStr;

// An event delivered to the sink. Rust owns it; call `bb_event_free` when done. `text` is valid
// until then.
typedef struct BbEvent {
  uint32_t kind;
  uint64_t id;
  uint64_t a;
  uint64_t b;
  struct BbStr text;
  const void *payload;
} BbEvent;

// The receiver of events. Called on Rust threads: enqueue and return, do nothing else.
typedef void (*BbEventSink)(void *ctx, struct BbEvent *evt);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

// The core version, as a view into static storage.
struct BbStr bb_version(void);

// The message behind the last `BB_ERR` or `BB_PANIC` on this thread. The view is valid until the next
// `bb_*` call on the same thread.
void bb_last_error(struct BbStr *out);

// Frees a string returned as `BbOwnedStr`.
void bb_str_free(struct BbOwnedStr s);

// Frees an event once the receiver is done with it.
void bb_event_free(struct BbEvent *evt);

// Emits `n` events of kind 1 (id = index, text = "event <index>") from a Rust thread, then one of
// kind 2 with text "done". Returns at once.
enum BbStatus bb_spike_emit(BbEventSink sink, void *ctx, uint32_t n);

// Upper-cases `input` into a string the caller owns.
enum BbStatus bb_spike_upper(struct BbStr input, struct BbOwnedStr *out);

// Always fails with a message, to prove `BB_ERR` and `bb_last_error`.
enum BbStatus bb_spike_fail(void);

// Always panics, to prove `BB_PANIC` and that nothing unwinds across the boundary.
enum BbStatus bb_spike_panic(void);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* BBCORE_H */

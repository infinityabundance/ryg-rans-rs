// rANS trace adapter -- emits JSON-line traces of rANS operations.
// Supports both 32-bit (byte) and 64-bit rANS variants.
// Uses the pinned upstream ryg rANS headers.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#include "platform.h"
#include "rans_byte.h"
#include "rans64.h"

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static uint32_t parse_u32(const char *s)
{
    unsigned long v = strtoul(s, NULL, 10);
    return (uint32_t)v;
}

static uint64_t parse_u64(const char *s)
{
    unsigned long long v = strtoull(s, NULL, 10);
    return (uint64_t)v;
}

static void usage(const char *prog)
{
    fprintf(stderr, "Usage: %s <operation> [params...]\n", prog);
    fprintf(stderr, "\n32-bit (byte) operations:\n");
    fprintf(stderr, "  enc-put-state      state start freq scale_bits\n");
    fprintf(stderr, "  enc-renorm         state freq scale_bits\n");
    fprintf(stderr, "  enc-flush          state\n");
    fprintf(stderr, "  dec-init           word\n");
    fprintf(stderr, "  dec-get            state scale_bits\n");
    fprintf(stderr, "  dec-advance        state start freq scale_bits\n");
    fprintf(stderr, "  enc-symbol-init    start freq scale_bits\n");
    fprintf(stderr, "  enc-put-symbol     state x_max rcp_freq rcp_shift bias cmpl_freq\n");
    fprintf(stderr, "\n64-bit operations:\n");
    fprintf(stderr, "  r64-enc-put-state  state start freq scale_bits\n");
    fprintf(stderr, "  r64-enc-renorm     state freq scale_bits\n");
    fprintf(stderr, "  r64-enc-flush      state\n");
    fprintf(stderr, "  r64-dec-init       word_lo word_hi\n");
    fprintf(stderr, "  r64-dec-get        state scale_bits\n");
    fprintf(stderr, "  r64-dec-advance    state start freq scale_bits\n");
    fprintf(stderr, "  r64-enc-symbol-init start freq scale_bits\n");
    fprintf(stderr, "  r64-enc-put-symbol state x_max rcp_freq rcp_shift bias cmpl_freq\n");
    fprintf(stderr, "  r64-mul-hi         a b\n");
    fprintf(stderr, "  r64-dec-renorm     state\n");
    exit(1);
}

// ===========================================================================
// 32-bit (byte) operations
// ===========================================================================

static void trace_enc_put_state(uint32_t state,
                                uint32_t start,
                                uint32_t freq,
                                uint32_t scale_bits)
{
    // Use a real buffer and call real RansEncPut
    RansState rans = state;
    uint8_t buf[16];
    uint8_t* ptr = buf + sizeof(buf);

    RansEncPut(&rans, &ptr, start, freq, scale_bits);
    uint32_t emitted = (uint32_t)((buf + sizeof(buf)) - ptr);

    printf("{\"op\":\"enc-put-state\""
           ",\"state_before\":%u"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"emitted\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, start, freq, scale_bits, emitted, rans);
}

static void trace_enc_renorm(uint32_t state,
                             uint32_t freq,
                             uint32_t scale_bits)
{
    // Use real RansEncRenorm
    uint8_t buf[16];
    uint8_t* ptr = buf + sizeof(buf);

    uint32_t threshold = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    RansState x = RansEncRenorm(state, &ptr, freq, scale_bits);
    uint32_t emitted = (uint32_t)((buf + sizeof(buf)) - ptr);

    printf("{\"op\":\"enc-renorm\""
           ",\"state_before\":%u"
           ",\"threshold\":%u"
           ",\"emitted_bytes\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, threshold, emitted, x);
}

static void trace_enc_flush(uint32_t state)
{
    // Use real RansEncFlush
    uint8_t buf[4];
    uint8_t* ptr = buf + sizeof(buf);
    RansState r = state;
    RansEncFlush(&r, &ptr);

    uint32_t w0 = ptr[0];
    uint32_t w1 = ptr[1];
    uint32_t w2 = ptr[2];
    uint32_t w3 = ptr[3];

    printf("{\"op\":\"enc-flush\""
           ",\"state\":%u"
           ",\"word0\":%u"
           ",\"word1\":%u"
           ",\"word2\":%u"
           ",\"word3\":%u"
           "}\n",
           state, w0, w1, w2, w3);
}

static void trace_dec_init(uint32_t word)
{
    // Use real RansDecInit
    uint8_t buf[4];
    buf[0] = (uint8_t)(word >> 0);
    buf[1] = (uint8_t)(word >> 8);
    buf[2] = (uint8_t)(word >> 16);
    buf[3] = (uint8_t)(word >> 24);
    uint8_t* ptr = buf;
    RansState r;
    RansDecInit(&r, &ptr);

    printf("{\"op\":\"dec-init\""
           ",\"word\":%u"
           ",\"state_after\":%u"
           "}\n", word, r);
}

static void trace_dec_get(uint32_t state, uint32_t scale_bits)
{
    uint32_t mask    = (1u << scale_bits) - 1;
    uint32_t cum_freq = RansDecGet(&state, scale_bits);

    printf("{\"op\":\"dec-get\""
           ",\"state\":%u"
           ",\"mask\":%u"
           ",\"cumulative_freq\":%u"
           "}\n", state, mask, cum_freq);
}

static void trace_dec_advance(uint32_t state,
                              uint32_t start,
                              uint32_t freq,
                              uint32_t scale_bits)
{
    uint32_t mask = (1u << scale_bits) - 1;

    // Use RansDecAdvanceStep to get the x value before renormalization
    RansState r_step = state;
    RansDecAdvanceStep(&r_step, start, freq, scale_bits);
    uint32_t x_before_renorm = r_step;

    // Use real RansDecAdvance with a buffer large enough for renormalization
    // The decoder reads bytes from the buffer if the state falls below RANS_BYTE_L.
    // Fill with 0xff so renormalization terminates quickly.
    uint8_t buf[16];
    memset(buf, 0xff, sizeof(buf));
    uint8_t* ptr = buf;
    uint8_t* ptr_before = ptr;
    RansState r = state;
    RansDecAdvance(&r, &ptr, start, freq, scale_bits);
    uint32_t bytes_consumed = (uint32_t)(ptr - ptr_before);

    printf("{\"op\":\"dec-advance\""
           ",\"state_before\":%u"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"mask\":%u"
           ",\"x_before_renorm\":%u"
           ",\"bytes_consumed\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, start, freq, scale_bits,
           mask, x_before_renorm, bytes_consumed, r);
}

static void trace_enc_symbol_init(uint32_t start,
                                  uint32_t freq,
                                  uint32_t scale_bits)
{
    // Use real RansEncSymbolInit, then serialize the struct fields
    RansEncSymbol sym;
    RansEncSymbolInit(&sym, start, freq, scale_bits);

    printf("{\"op\":\"enc-symbol-init\""
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"x_max\":%u"
           ",\"rcp_freq\":%u"
           ",\"bias\":%u"
           ",\"cmpl_freq\":%u"
           ",\"rcp_shift\":%u"
           "}\n",
           start, freq, scale_bits,
           sym.x_max, sym.rcp_freq, sym.bias, sym.cmpl_freq, sym.rcp_shift);
}

static void trace_enc_put_symbol(uint32_t state,
                                 uint32_t x_max,
                                 uint32_t rcp_freq,
                                 uint32_t rcp_shift,
                                 uint32_t bias,
                                 uint32_t cmpl_freq)
{
    // Reconstruct the symbol struct from the serialized fields, then
    // call real RansEncPutSymbol on a real buffer.
    RansEncSymbol sym;
    sym.x_max     = x_max;
    sym.rcp_freq  = rcp_freq;
    sym.bias      = bias;
    sym.cmpl_freq = (uint16_t)cmpl_freq;
    sym.rcp_shift = (uint16_t)rcp_shift;

    uint8_t buf[16];
    uint8_t* ptr = buf + sizeof(buf);
    RansState r = state;
    RansEncPutSymbol(&r, &ptr, &sym);
    uint32_t emitted = (uint32_t)((buf + sizeof(buf)) - ptr);

    // Compute q for trace output (not returned by the upstream function)
    uint32_t x = state;
    if (x >= x_max) {
        do { x >>= 8; } while (x >= x_max);
    }
    uint32_t q = (uint32_t)(((uint64_t)x * rcp_freq) >> 32) >> rcp_shift;

    printf("{\"op\":\"enc-put-symbol\""
           ",\"state_before\":%u"
           ",\"x_max\":%u"
           ",\"emitted_bytes\":%u"
           ",\"q\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, x_max, emitted, q, r);
}

// ===========================================================================
// 64-bit operations  (using rans64.h)
// ===========================================================================

static void trace_r64_enc_put_state(uint64_t state,
                                    uint32_t start,
                                    uint32_t freq,
                                    uint32_t scale_bits)
{
    // Use a real buffer and call real Rans64EncPut
    Rans64State rans = state;
    uint32_t buf[16];
    uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);

    Rans64EncPut(&rans, &ptr, start, freq, scale_bits);
    uint32_t emitted = (uint32_t)((buf + sizeof(buf) / sizeof(buf[0])) - ptr);

    printf("{\"op\":\"r64-enc-put-state\""
           ",\"state_before\":%llu"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"emitted\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state, start, freq, scale_bits,
           emitted,
           (unsigned long long)rans);
}

static void trace_r64_enc_renorm(uint64_t state,
                                 uint32_t freq,
                                 uint32_t scale_bits)
{
    // No separate Rans64EncRenorm exists -- the renormalization is inlined
    // in Rans64EncPut.  Keep the formula for this case.
    uint64_t x        = state;
    uint64_t threshold = ((RANS64_L >> scale_bits) << 32) * (uint64_t)freq;
    uint32_t emitted  = 0;
    if (x >= threshold) {
        emitted++;
        x >>= 32;
    }
    printf("{\"op\":\"r64-enc-renorm\""
           ",\"state_before\":%llu"
           ",\"threshold\":%llu"
           ",\"emitted_words\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state,
           (unsigned long long)threshold,
           emitted,
           (unsigned long long)x);
}

static void trace_r64_enc_flush(uint64_t state)
{
    // Use real Rans64EncFlush
    uint32_t buf[2];
    uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
    Rans64State r = state;
    Rans64EncFlush(&r, &ptr);

    uint32_t w0 = ptr[0];
    uint32_t w1 = ptr[1];

    printf("{\"op\":\"r64-enc-flush\""
           ",\"state\":%llu"
           ",\"word0\":%u"
           ",\"word1\":%u"
           "}\n",
           (unsigned long long)state, w0, w1);
}

static void trace_r64_dec_init(uint32_t word_lo, uint32_t word_hi)
{
    // Use real Rans64DecInit
    uint32_t buf[2];
    buf[0] = word_lo;
    buf[1] = word_hi;
    uint32_t* ptr = buf;
    Rans64State r;
    Rans64DecInit(&r, &ptr);

    printf("{\"op\":\"r64-dec-init\""
           ",\"word_lo\":%u"
           ",\"word_hi\":%u"
           ",\"state_after\":%llu"
           "}\n",
           word_lo, word_hi, (unsigned long long)r);
}

static void trace_r64_dec_get(uint64_t state, uint32_t scale_bits)
{
    uint64_t mask    = (1ull << scale_bits) - 1;
    uint64_t cum_freq = Rans64DecGet(&state, scale_bits);

    printf("{\"op\":\"r64-dec-get\""
           ",\"state\":%llu"
           ",\"mask\":%llu"
           ",\"cumulative_freq\":%llu"
           "}\n",
           (unsigned long long)state,
           (unsigned long long)mask,
           (unsigned long long)cum_freq);
}

static void trace_r64_dec_advance(uint64_t state,
                                  uint32_t start,
                                  uint32_t freq,
                                  uint32_t scale_bits)
{
    uint64_t mask = (1ull << scale_bits) - 1;

    // Use Rans64DecAdvanceStep to get x before renormalization
    Rans64State r_step = state;
    Rans64DecAdvanceStep(&r_step, start, freq, scale_bits);
    uint64_t x_before_renorm = r_step;

    // Use real Rans64DecAdvance with a buffer for renormalization
    uint32_t buf[16];
    memset(buf, 0xff, sizeof(buf));
    uint32_t* ptr = buf;
    uint32_t* ptr_before = ptr;
    Rans64State r = state;
    Rans64DecAdvance(&r, &ptr, start, freq, scale_bits);
    uint32_t words_consumed = (uint32_t)(ptr - ptr_before);

    printf("{\"op\":\"r64-dec-advance\""
           ",\"state_before\":%llu"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"mask\":%llu"
           ",\"x_before_renorm\":%llu"
           ",\"words_consumed\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state, start, freq, scale_bits,
           (unsigned long long)mask,
           (unsigned long long)x_before_renorm,
           words_consumed,
           (unsigned long long)r);
}

static void trace_r64_enc_symbol_init(uint32_t start,
                                      uint32_t freq,
                                      uint32_t scale_bits)
{
    // Use real Rans64EncSymbolInit, then serialize the struct fields
    Rans64EncSymbol sym;
    Rans64EncSymbolInit(&sym, start, freq, scale_bits);

    // x_max is not stored in Rans64EncSymbol, compute it for trace output
    uint64_t x_max = ((RANS64_L >> scale_bits) << 32) * (uint64_t)freq;

    printf("{\"op\":\"r64-enc-symbol-init\""
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"x_max\":%llu"
           ",\"rcp_freq\":%llu"
           ",\"bias\":%u"
           ",\"cmpl_freq\":%u"
           ",\"rcp_shift\":%u"
           "}\n",
           start, freq, scale_bits,
           (unsigned long long)x_max,
           (unsigned long long)sym.rcp_freq,
           sym.bias, sym.cmpl_freq, sym.rcp_shift);
}

static void trace_r64_enc_put_symbol(uint64_t state,
                                     uint64_t x_max,
                                     uint64_t rcp_freq,
                                     uint32_t rcp_shift,
                                     uint32_t bias,
                                     uint32_t cmpl_freq)
{
    // We cannot call Rans64EncPutSymbol because it recomputes x_max from
    // sym->freq internally, but we don't know freq (only cmpl_freq, which
    // also requires scale_bits to recover freq).  The core arithmetic uses
    // the upstream Rans64MulHi for the multiply-high part.

    uint64_t x       = state;
    uint32_t emitted = 0;
    if (x >= x_max) {
        emitted++;
        x >>= 32;
    }
    uint64_t q           = Rans64MulHi(x, rcp_freq) >> rcp_shift;
    uint64_t state_after = x + bias + q * cmpl_freq;

    printf("{\"op\":\"r64-enc-put-symbol\""
           ",\"state_before\":%llu"
           ",\"x_max\":%llu"
           ",\"emitted_words\":%u"
           ",\"q\":%llu"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state,
           (unsigned long long)x_max,
           emitted,
           (unsigned long long)q,
           (unsigned long long)state_after);
}

static void trace_r64_mul_hi(uint64_t a, uint64_t b)
{
    uint64_t result = Rans64MulHi(a, b);
    printf("{\"op\":\"r64-mul-hi\""
           ",\"a\":%llu"
           ",\"b\":%llu"
           ",\"result\":%llu"
           "}\n",
           (unsigned long long)a,
           (unsigned long long)b,
           (unsigned long long)result);
}

static void trace_r64_dec_renorm(uint64_t state)
{
    // Use real Rans64DecRenorm
    uint32_t buf[16];
    memset(buf, 0xff, sizeof(buf));
    uint32_t* ptr = buf;
    uint32_t* ptr_before = ptr;
    Rans64State r = state;
    Rans64DecRenorm(&r, &ptr);
    uint32_t words_consumed = (uint32_t)(ptr - ptr_before);

    printf("{\"op\":\"r64-dec-renorm\""
           ",\"state_before\":%llu"
           ",\"words_consumed\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state, words_consumed,
           (unsigned long long)r);
}

// ===========================================================================
// Main dispatch
// ===========================================================================

int main(int argc, char *argv[])
{
    if (argc < 2) usage(argv[0]);

    const char *op = argv[1];

    // ---- 32-bit operations ----
    if (strcmp(op, "enc-put-state") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_enc_put_state(parse_u32(argv[2]),
                            parse_u32(argv[3]),
                            parse_u32(argv[4]),
                            parse_u32(argv[5]));
    } else if (strcmp(op, "enc-renorm") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_renorm(parse_u32(argv[2]),
                         parse_u32(argv[3]),
                         parse_u32(argv[4]));
    } else if (strcmp(op, "enc-flush") == 0) {
        if (argc != 3) usage(argv[0]);
        trace_enc_flush(parse_u32(argv[2]));
    } else if (strcmp(op, "dec-init") == 0) {
        if (argc != 3) usage(argv[0]);
        trace_dec_init(parse_u32(argv[2]));
    } else if (strcmp(op, "dec-get") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_dec_get(parse_u32(argv[2]),
                      parse_u32(argv[3]));
    } else if (strcmp(op, "dec-advance") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_dec_advance(parse_u32(argv[2]),
                          parse_u32(argv[3]),
                          parse_u32(argv[4]),
                          parse_u32(argv[5]));
    } else if (strcmp(op, "enc-symbol-init") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_symbol_init(parse_u32(argv[2]),
                              parse_u32(argv[3]),
                              parse_u32(argv[4]));
    } else if (strcmp(op, "enc-put-symbol") == 0) {
        if (argc != 8) usage(argv[0]);
        trace_enc_put_symbol(parse_u32(argv[2]),
                             parse_u32(argv[3]),
                             parse_u32(argv[4]),
                             parse_u32(argv[5]),
                             parse_u32(argv[6]),
                             parse_u32(argv[7]));

    // ---- 64-bit operations ----
    } else if (strcmp(op, "r64-enc-put-state") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_r64_enc_put_state(parse_u64(argv[2]),
                                parse_u32(argv[3]),
                                parse_u32(argv[4]),
                                parse_u32(argv[5]));
    } else if (strcmp(op, "r64-enc-renorm") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_r64_enc_renorm(parse_u64(argv[2]),
                             parse_u32(argv[3]),
                             parse_u32(argv[4]));
    } else if (strcmp(op, "r64-enc-flush") == 0) {
        if (argc != 3) usage(argv[0]);
        trace_r64_enc_flush(parse_u64(argv[2]));
    } else if (strcmp(op, "r64-dec-init") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_r64_dec_init(parse_u32(argv[2]),
                           parse_u32(argv[3]));
    } else if (strcmp(op, "r64-dec-get") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_r64_dec_get(parse_u64(argv[2]),
                          parse_u32(argv[3]));
    } else if (strcmp(op, "r64-dec-advance") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_r64_dec_advance(parse_u64(argv[2]),
                              parse_u32(argv[3]),
                              parse_u32(argv[4]),
                              parse_u32(argv[5]));
    } else if (strcmp(op, "r64-enc-symbol-init") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_r64_enc_symbol_init(parse_u32(argv[2]),
                                  parse_u32(argv[3]),
                                  parse_u32(argv[4]));
    } else if (strcmp(op, "r64-enc-put-symbol") == 0) {
        if (argc != 8) usage(argv[0]);
        trace_r64_enc_put_symbol(parse_u64(argv[2]),
                                 parse_u64(argv[3]),
                                 parse_u64(argv[4]),
                                 parse_u32(argv[5]),
                                 parse_u32(argv[6]),
                                 parse_u32(argv[7]));
    } else if (strcmp(op, "r64-mul-hi") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_r64_mul_hi(parse_u64(argv[2]),
                         parse_u64(argv[3]));
    } else if (strcmp(op, "r64-dec-renorm") == 0) {
        if (argc != 3) usage(argv[0]);
        trace_r64_dec_renorm(parse_u64(argv[2]));
    } else {
        fprintf(stderr, "Unknown operation: %s\n", op);
        usage(argv[0]);
    }

    return 0;
}

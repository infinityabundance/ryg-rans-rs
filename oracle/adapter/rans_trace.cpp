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
    uint32_t state_after = ((state / freq) << scale_bits) + (state % freq) + start;
    printf("{\"op\":\"enc-put-state\""
           ",\"state_before\":%u"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, start, freq, scale_bits, state_after);
}

static void trace_enc_renorm(uint32_t state,
                             uint32_t freq,
                             uint32_t scale_bits)
{
    uint32_t x        = state;
    uint32_t threshold = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    uint32_t emitted  = 0;
    if (x >= threshold) {
        do { emitted++; x >>= 8; } while (x >= threshold);
    }
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
    uint32_t w0 = (state >> 0) & 0xff;
    uint32_t w1 = (state >> 8) & 0xff;
    uint32_t w2 = (state >> 16) & 0xff;
    uint32_t w3 = (state >> 24) & 0xff;
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
    printf("{\"op\":\"dec-init\""
           ",\"word\":%u"
           ",\"state_after\":%u"
           "}\n", word, word);
}

static void trace_dec_get(uint32_t state, uint32_t scale_bits)
{
    uint32_t mask    = (1u << scale_bits) - 1;
    uint32_t cum_freq = state & mask;
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
    uint32_t mask           = (1u << scale_bits) - 1;
    uint32_t x              = state;
    x                       = freq * (x >> scale_bits) + (x & mask) - start;
    uint32_t x_before_renorm = x;
    uint32_t bytes_consumed  = 0;
    if (x < RANS_BYTE_L) {
        do { bytes_consumed++; x = (x << 8); } while (x < RANS_BYTE_L);
    }
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
           mask, x_before_renorm, bytes_consumed, x);
}

static void trace_enc_symbol_init(uint32_t start,
                                  uint32_t freq,
                                  uint32_t scale_bits)
{
    uint32_t x_max    = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    uint32_t cmpl_freq = (1u << scale_bits) - freq;
    uint32_t rcp_freq, bias, rcp_shift;

    if (freq < 2) {
        rcp_freq  = ~0u;
        rcp_shift = 0;
        bias      = start + (1u << scale_bits) - 1;
    } else {
        uint32_t shift = 0;
        while (freq > (1u << shift)) shift++;
        rcp_freq  = (uint32_t)(((1ull << (shift + 31)) + freq - 1) / freq);
        rcp_shift = shift - 1;
        bias      = start;
    }

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
           x_max, rcp_freq, bias, cmpl_freq, rcp_shift);
}

static void trace_enc_put_symbol(uint32_t state,
                                 uint32_t x_max,
                                 uint32_t rcp_freq,
                                 uint32_t rcp_shift,
                                 uint32_t bias,
                                 uint32_t cmpl_freq)
{
    uint32_t x       = state;
    uint32_t emitted = 0;
    if (x >= x_max) {
        do { emitted++; x >>= 8; } while (x >= x_max);
    }
    uint32_t q           = (uint32_t)(((uint64_t)x * rcp_freq) >> 32) >> rcp_shift;
    uint32_t state_after = x + bias + q * cmpl_freq;

    printf("{\"op\":\"enc-put-symbol\""
           ",\"state_before\":%u"
           ",\"x_max\":%u"
           ",\"emitted_bytes\":%u"
           ",\"q\":%u"
           ",\"state_after\":%u"
           "}\n",
           state, x_max, emitted, q, state_after);
}

// ===========================================================================
// 64-bit operations  (using rans64.h)
// ===========================================================================

static void trace_r64_enc_put_state(uint64_t state,
                                    uint32_t start,
                                    uint32_t freq,
                                    uint32_t scale_bits)
{
    uint64_t state_after = ((state / freq) << scale_bits) + (state % freq) + start;
    printf("{\"op\":\"r64-enc-put-state\""
           ",\"state_before\":%llu"
           ",\"start\":%u"
           ",\"freq\":%u"
           ",\"scale_bits\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state, start, freq, scale_bits,
           (unsigned long long)state_after);
}

static void trace_r64_enc_renorm(uint64_t state,
                                 uint32_t freq,
                                 uint32_t scale_bits)
{
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
    uint32_t w0 = (uint32_t)(state >> 0);
    uint32_t w1 = (uint32_t)(state >> 32);
    printf("{\"op\":\"r64-enc-flush\""
           ",\"state\":%llu"
           ",\"word0\":%u"
           ",\"word1\":%u"
           "}\n",
           (unsigned long long)state, w0, w1);
}

static void trace_r64_dec_init(uint32_t word_lo, uint32_t word_hi)
{
    uint64_t state = ((uint64_t)word_hi << 32) | word_lo;
    printf("{\"op\":\"r64-dec-init\""
           ",\"word_lo\":%u"
           ",\"word_hi\":%u"
           ",\"state_after\":%llu"
           "}\n",
           word_lo, word_hi, (unsigned long long)state);
}

static void trace_r64_dec_get(uint64_t state, uint32_t scale_bits)
{
    uint64_t mask    = (1ull << scale_bits) - 1;
    uint64_t cum_freq = state & mask;
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
    uint64_t state_before    = state;
    uint64_t mask            = (1ull << scale_bits) - 1;
    uint64_t x               = state;
    x                        = freq * (x >> scale_bits) + (x & mask) - start;
    uint64_t x_before_renorm = x;
    uint32_t words_consumed  = 0;
    if (x < RANS64_L) {
        words_consumed++;
        x = (x << 32);
    }
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
           (unsigned long long)state_before, start, freq, scale_bits,
           (unsigned long long)mask,
           (unsigned long long)x_before_renorm,
           words_consumed,
           (unsigned long long)x);
}

static void trace_r64_enc_symbol_init(uint32_t start,
                                      uint32_t freq,
                                      uint32_t scale_bits)
{
    uint64_t x_max    = ((RANS64_L >> scale_bits) << 32) * (uint64_t)freq;
    uint32_t cmpl_freq = ((1u << scale_bits) - freq);
    uint32_t bias;
    uint64_t rcp_freq;
    uint32_t rcp_shift;

    if (freq < 2) {
        rcp_freq  = ~0ull;
        rcp_shift = 0;
        bias      = start + (1u << scale_bits) - 1;
    } else {
        uint32_t shift = 0;
        uint64_t x0, x1, t0, t1;
        while (freq > (1u << shift)) shift++;
        x0 = freq - 1;
        x1 = 1ull << (shift + 31);
        t1 = x1 / freq;
        x0 += (x1 % freq) << 32;
        t0 = x0 / freq;
        rcp_freq  = t0 + (t1 << 32);
        rcp_shift = shift - 1;
        bias      = start;
    }

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
           (unsigned long long)rcp_freq,
           bias, cmpl_freq, rcp_shift);
}

static void trace_r64_enc_put_symbol(uint64_t state,
                                     uint64_t x_max,
                                     uint64_t rcp_freq,
                                     uint32_t rcp_shift,
                                     uint32_t bias,
                                     uint32_t cmpl_freq)
{
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
    uint64_t state_before = state;
    uint32_t words_consumed = 0;
    if (state < RANS64_L) {
        words_consumed++;
        state = (state << 32);
    }
    printf("{\"op\":\"r64-dec-renorm\""
           ",\"state_before\":%llu"
           ",\"words_consumed\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state_before, words_consumed,
           (unsigned long long)state);
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

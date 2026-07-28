// rANS trace adapter -- emits JSON-line traces of rANS operations.
// Supports both 32-bit (byte) and 64-bit rANS variants.
// Uses the pinned upstream ryg rANS headers.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <vector>
#include <string>

#include "platform.h"
#include "rans_byte.h"
#include "rans64.h"
#include "rans_word_sse41.h"

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
    fprintf(stderr, "  r64-enc-put-symbol state start freq scale_bits\n");
    fprintf(stderr, "  r64-mul-hi         a b\n");
    fprintf(stderr, "  r64-dec-renorm     state\n");
    fprintf(stderr, "\nStream operations (full encode/decode):\n");
    fprintf(stderr, "  enc-stream-byte    scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-byte    scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-r64     scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-r64     scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-byte-div scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  enc-stream-r64-div  scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-byte-div scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  dec-stream-r64-div  scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-byte-interleaved2      scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  enc-stream-byte-interleaved2-div   scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-byte-interleaved2      scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  dec-stream-byte-interleaved2-div   scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-r64-interleaved2       scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  enc-stream-r64-interleaved2-div    scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-r64-interleaved2       scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  dec-stream-r64-interleaved2-div    scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-word                   scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-word                   scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-word-interleaved2       scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-word-interleaved2       scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "\nAlias operations (alias method, byte rANS):\n");
    fprintf(stderr, "  trace-alias-table                  scale_bits freq_csv\n");
    fprintf(stderr, "  enc-stream-alias                  scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-alias                  scale_bits freq_csv compressed_hex num_symbols\n");
    fprintf(stderr, "  enc-stream-alias-interleaved2     scale_bits freq_csv input_hex\n");
    fprintf(stderr, "  dec-stream-alias-interleaved2     scale_bits freq_csv compressed_hex num_symbols\n");
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
                                     uint32_t start,
                                     uint32_t freq,
                                     uint32_t scale_bits)
{
    // Use real Rans64EncSymbolInit + Rans64EncPutSymbol
    Rans64EncSymbol sym;
    Rans64EncSymbolInit(&sym, start, freq, scale_bits);

    // x_max is not stored in Rans64EncSymbol, compute it for trace output
    uint64_t x_max = ((RANS64_L >> scale_bits) << 32) * (uint64_t)freq;

    Rans64State state_after = state;
    uint32_t buf[16];
    uint32_t *ptr = buf + 16;
    uint32_t *ptr_before = ptr;

    // Call the real upstream function
    Rans64EncPutSymbol(&state_after, &ptr, &sym, scale_bits);
    uint32_t emitted = (uint32_t)(ptr_before - ptr);  // words consumed from end

    printf("{\"op\":\"r64-enc-put-symbol\""
           ",\"state_before\":%llu"
           ",\"x_max\":%llu"
           ",\"rcp_freq\":%llu"
           ",\"rcp_shift\":%u"
           ",\"bias\":%u"
           ",\"cmpl_freq\":%u"
           ",\"emitted_words\":%u"
           ",\"state_after\":%llu"
           "}\n",
           (unsigned long long)state,
           (unsigned long long)x_max,
           (unsigned long long)sym.rcp_freq,
           sym.rcp_shift,
           sym.bias,
           sym.cmpl_freq,
           emitted,
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
// Hex helpers
// ===========================================================================

static uint8_t hex_nibble(char c)
{
    if (c >= '0' && c <= '9') return (uint8_t)(c - '0');
    if (c >= 'a' && c <= 'f') return (uint8_t)(c - 'a' + 10);
    if (c >= 'A' && c <= 'F') return (uint8_t)(c - 'A' + 10);
    return 0;
}

static std::vector<uint8_t> hex_decode(const char* hex)
{
    size_t len = strlen(hex);
    std::vector<uint8_t> out;
    for (size_t i = 0; i + 1 < len; i += 2) {
        out.push_back((hex_nibble(hex[i]) << 4) | hex_nibble(hex[i+1]));
    }
    return out;
}

static std::string hex_encode(const uint8_t* data, size_t len)
{
    static const char hex[] = "0123456789abcdef";
    std::string out;
    for (size_t i = 0; i < len; i++) {
        out += hex[data[i] >> 4];
        out += hex[data[i] & 0xf];
    }
    return out;
}

static std::vector<uint32_t> parse_freq_csv(const char* csv)
{
    std::vector<uint32_t> freqs;
    const char* p = csv;
    while (*p) {
        unsigned long v = strtoul(p, NULL, 10);
        freqs.push_back((uint32_t)v);
        while (*p && *p != ',') p++;
        if (*p == ',') p++;
    }
    return freqs;
}

// ===========================================================================
// Stream operations (full encode/decode for cross-decoding courts)
// ===========================================================================

// enc-stream-byte: encode a full input using byte rANS with given frequencies
// Usage: enc-stream-byte scale_bits freq_csv input_hex
//   freq_csv: 256 comma-separated frequencies summing to 1<<scale_bits
//   input_hex: hex-encoded input symbols
// Output: JSON with hex-encoded compressed data and metadata
static void trace_enc_stream_byte(uint32_t scale_bits,
                                  const std::vector<uint32_t>& freqs,
                                  const std::vector<uint8_t>& input)
{
    uint32_t total = 1u << scale_bits;
    
    // Build cumulative frequencies and symbols
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }
    
    // Precompute encoder symbols
    RansEncSymbol esyms[256];
    RansDecSymbol dsyms[256];
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansEncSymbolInit(&esyms[i], cum_freqs[i], freqs[i], scale_bits);
            RansDecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
        }
    }
    
    // Encode
    uint8_t buf[64 * 1024];
    uint8_t* ptr = buf + sizeof(buf);
    RansState state;
    RansEncInit(&state);
    
    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i-1];
        RansEncPutSymbol(&state, &ptr, &esyms[s]);
    }
    RansEncFlush(&state, &ptr);
    
    size_t comp_size = sizeof(buf) - (ptr - buf);
    std::string comp_hex = hex_encode(ptr, comp_size);
    
    // Decode back to verify
    uint8_t* dec_ptr = ptr;
    RansState dec_state;
    RansDecInit(&dec_state, &dec_ptr);
    
    std::vector<uint8_t> decoded(input.size());
    for (size_t i = 0; i < input.size(); i++) {
        uint32_t cf = RansDecGet(&dec_state, scale_bits);
        // Brute-force symbol lookup
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        decoded[i] = (uint8_t)s;
        RansDecAdvanceSymbol(&dec_state, &dec_ptr, &dsyms[s], scale_bits);
    }
    
    bool decode_ok = (decoded == input);
    
    printf("{\"op\":\"enc-stream-byte\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_size\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"decode_ok\":%s"
           ",\"final_state\":%u"
           "}\n",
           scale_bits,
           input.size(),
           comp_size,
           comp_hex.c_str(),
           decode_ok ? "true" : "false",
           dec_state);
}

// dec-stream-byte: decode a compressed stream using byte rANS
static void trace_dec_stream_byte(uint32_t scale_bits,
                                  const std::vector<uint32_t>& freqs,
                                  const std::vector<uint8_t>& compressed,
                                  size_t num_symbols)
{
    uint32_t total = 1u << scale_bits;
    
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }
    
    RansDecSymbol dsyms[256];
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansDecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
        }
    }
    
    // Prepare mutable buffer (decoder advances the pointer)
    std::vector<uint8_t> buf(compressed.begin(), compressed.end());
    uint8_t* ptr = buf.data();
    
    RansState state;
    RansDecInit(&state, &ptr);
    
    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        uint32_t cf = RansDecGet(&state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        output[i] = (uint8_t)s;
        RansDecAdvanceSymbol(&state, &ptr, &dsyms[s], scale_bits);
    }
    
    printf("{\"op\":\"dec-stream-byte\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           ",\"consumed\":%zu"
           "}\n",
           scale_bits,
           num_symbols,
           hex_encode(output.data(), output.size()).c_str(),
           (size_t)(ptr - buf.data()));
}

// enc-stream-r64: encode using 64-bit rANS
static void trace_enc_stream_r64(uint32_t scale_bits,
                                 const std::vector<uint32_t>& freqs,
                                 const std::vector<uint8_t>& input)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }
    
    Rans64EncSymbol esyms[256];
    Rans64DecSymbol dsyms[256];
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            Rans64EncSymbolInit(&esyms[i], cum_freqs[i], freqs[i], scale_bits);
            Rans64DecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
        }
    }
    
    uint32_t buf[64 * 1024];
    uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
    Rans64State state;
    Rans64EncInit(&state);
    
    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i-1];
        Rans64EncPutSymbol(&state, &ptr, &esyms[s], scale_bits);
    }
    Rans64EncFlush(&state, &ptr);
    
    size_t comp_words = (sizeof(buf) / sizeof(buf[0])) - (ptr - buf);
    size_t comp_bytes = comp_words * sizeof(uint32_t);
    
    // Decode back to verify
    uint32_t* dec_ptr = ptr;
    Rans64State dec_state;
    Rans64DecInit(&dec_state, &dec_ptr);
    
    std::vector<uint8_t> decoded(input.size());
    for (size_t i = 0; i < input.size(); i++) {
        uint32_t cf = Rans64DecGet(&dec_state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        decoded[i] = (uint8_t)s;
        Rans64DecAdvanceSymbol(&dec_state, &dec_ptr, &dsyms[s], scale_bits);
    }
    
    bool decode_ok = (decoded == input);
    std::string comp_hex = hex_encode((const uint8_t*)ptr, comp_bytes);
    
    printf("{\"op\":\"enc-stream-r64\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_words\":%zu"
           ",\"compressed_bytes\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"decode_ok\":%s"
           "}\n",
           scale_bits,
           input.size(),
           comp_words,
           comp_bytes,
           comp_hex.c_str(),
           decode_ok ? "true" : "false");
}

// dec-stream-r64: decode using 64-bit rANS
static void trace_dec_stream_r64(uint32_t scale_bits,
                                 const std::vector<uint32_t>& freqs,
                                 const std::vector<uint8_t>& compressed,
                                 size_t num_symbols)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }
    
    Rans64DecSymbol dsyms[256];
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            Rans64DecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
        }
    }
    
    // 64-bit decoder reads uint32_t words from the stream.
    // Ensure compressed size is a multiple of 4.
    std::vector<uint32_t> words(compressed.size() / 4 + 1, 0);
    memcpy(words.data(), compressed.data(), compressed.size());
    uint32_t* ptr = words.data();
    
    Rans64State state;
    Rans64DecInit(&state, &ptr);
    
    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        uint32_t cf = Rans64DecGet(&state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        output[i] = (uint8_t)s;
        Rans64DecAdvanceSymbol(&state, &ptr, &dsyms[s], scale_bits);
    }
    
    printf("{\"op\":\"dec-stream-r64\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           ",\"consumed_words\":%zu"
           "}\n",
           scale_bits,
           num_symbols,
           hex_encode(output.data(), output.size()).c_str(),
           (size_t)(ptr - words.data()));
}

// ===========================================================================
// Division-mode stream operations (use RansEncPut instead of RansEncPutSymbol)
// ===========================================================================

static void trace_enc_stream_byte_div(uint32_t scale_bits,
                                      const std::vector<uint32_t>& freqs,
                                      const std::vector<uint8_t>& input)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    // Encode using division path
    uint8_t buf[64 * 1024];
    uint8_t* ptr = buf + sizeof(buf);
    RansState state;
    RansEncInit(&state);

    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i-1];
        RansEncPut(&state, &ptr, cum_freqs[s], freqs[s], scale_bits);
    }
    RansEncFlush(&state, &ptr);

    size_t comp_size = sizeof(buf) - (ptr - buf);
    std::string comp_hex = hex_encode(ptr, comp_size);

    // Self-decode to verify
    uint8_t* dec_ptr = ptr;
    RansState dec_state;
    RansDecInit(&dec_state, &dec_ptr);
    bool decode_ok = true;
    for (size_t i = 0; i < input.size(); i++) {
        uint32_t cf = RansDecGet(&dec_state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        if ((uint8_t)s != input[i]) { decode_ok = false; break; }
        RansDecAdvance(&dec_state, &dec_ptr, cum_freqs[s], freqs[s], scale_bits);
    }

    printf("{\"op\":\"enc-stream-byte-div\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_size\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"decode_ok\":%s"
           "}\n",
           scale_bits, input.size(), comp_size, comp_hex.c_str(),
           decode_ok ? "true" : "false");
}

static void trace_enc_stream_r64_div(uint32_t scale_bits,
                                     const std::vector<uint32_t>& freqs,
                                     const std::vector<uint8_t>& input)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    uint32_t buf[64 * 1024];
    uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
    Rans64State state;
    Rans64EncInit(&state);

    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i-1];
        Rans64EncPut(&state, &ptr, cum_freqs[s], freqs[s], scale_bits);
    }
    Rans64EncFlush(&state, &ptr);

    size_t comp_words = (sizeof(buf) / sizeof(buf[0])) - (ptr - buf);
    size_t comp_bytes = comp_words * sizeof(uint32_t);
    std::string comp_hex = hex_encode((const uint8_t*)ptr, comp_bytes);

    // Self-decode to verify
    uint32_t* dec_ptr = ptr;
    Rans64State dec_state;
    Rans64DecInit(&dec_state, &dec_ptr);
    bool decode_ok = true;
    for (size_t i = 0; i < input.size(); i++) {
        uint32_t cf = Rans64DecGet(&dec_state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        if ((uint8_t)s != input[i]) { decode_ok = false; break; }
        Rans64DecAdvance(&dec_state, &dec_ptr, cum_freqs[s], freqs[s], scale_bits);
    }

    printf("{\"op\":\"enc-stream-r64-div\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_words\":%zu"
           ",\"compressed_bytes\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"decode_ok\":%s"
           "}\n",
           scale_bits, input.size(), comp_words, comp_bytes, comp_hex.c_str(),
           decode_ok ? "true" : "false");
}

// ---------------------------------------------------------------------------
// Division-mode decoder: use RansDecAdvance (not RansDecAdvanceSymbol)
// ---------------------------------------------------------------------------

static void trace_dec_stream_byte_div(uint32_t scale_bits,
                                      const std::vector<uint32_t>& freqs,
                                      const std::vector<uint8_t>& compressed,
                                      size_t num_symbols)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    std::vector<uint8_t> buf(compressed.begin(), compressed.end());
    uint8_t* ptr = buf.data();
    RansState state;
    RansDecInit(&state, &ptr);

    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        uint32_t cf = RansDecGet(&state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        output[i] = (uint8_t)s;
        // Use division-based RansDecAdvance
        RansDecAdvance(&state, &ptr, cum_freqs[s], freqs[s], scale_bits);
    }

    printf("{\"op\":\"dec-stream-byte-div\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           ",\"consumed\":%zu"
           "}\n",
           scale_bits, num_symbols,
           hex_encode(output.data(), output.size()).c_str(),
           (size_t)(ptr - buf.data()));
}

static void trace_dec_stream_r64_div(uint32_t scale_bits,
                                     const std::vector<uint32_t>& freqs,
                                     const std::vector<uint8_t>& compressed,
                                     size_t num_symbols)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    std::vector<uint32_t> words(compressed.size() / 4 + 1, 0);
    memcpy(words.data(), compressed.data(), compressed.size());
    uint32_t* ptr = words.data();
    Rans64State state;
    Rans64DecInit(&state, &ptr);

    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        uint32_t cf = Rans64DecGet(&state, scale_bits);
        int s = 0;
        for (int j = 0; j < 256; j++) {
            if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
        }
        output[i] = (uint8_t)s;
        // Use division-based Rans64DecAdvance
        Rans64DecAdvance(&state, &ptr, cum_freqs[s], freqs[s], scale_bits);
    }

    printf("{\"op\":\"dec-stream-r64-div\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           ",\"consumed_words\":%zu"
           "}\n",
           scale_bits, num_symbols,
           hex_encode(output.data(), output.size()).c_str(),
           (size_t)(ptr - words.data()));
}

// ===========================================================================
// Interleaved stream operations (two states)
// ===========================================================================

// ---------------------------------------------------------------------------
// Byte interleaved2 encode (reciprocal fast path)
// ---------------------------------------------------------------------------
// enc-stream-byte-interleaved2: reciprocal-symbol interleaved encode
static void trace_enc_stream_byte_interleaved2(uint32_t scale_bits,
                                               const std::vector<uint32_t>& freqs,
                                               const std::vector<uint8_t>& input)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   RansEncSymbol esyms[256];
   for (int i = 0; i < 256; i++) {
       if (freqs[i] > 0) {
           RansEncSymbolInit(&esyms[i], cum_freqs[i], freqs[i], scale_bits);
       }
   }
   uint8_t buf[64 * 1024];
   uint8_t* ptr = buf + sizeof(buf);
   RansState state0, state1;
   RansEncInit(&state0);
   RansEncInit(&state1);
   int count = (int)input.size();
   if (count & 1) {
       int s = input[count - 1];
       RansEncPutSymbol(&state0, &ptr, &esyms[s]);
   }
   for (int i = count & ~1; i > 0; i -= 2) {
       int s1 = input[i - 1];
       int s0 = input[i - 2];
       RansEncPutSymbol(&state1, &ptr, &esyms[s1]);
       RansEncPutSymbol(&state0, &ptr, &esyms[s0]);
   }
   RansEncFlush(&state1, &ptr);
   RansEncFlush(&state0, &ptr);
   size_t comp_size = sizeof(buf) - (ptr - buf);
   printf("{\"op\":\"enc-stream-byte-interleaved2\""
          ",\"scale_bits\":%u"
          ",\"compressed_hex\":\"%s\""
          ",\"compressed_size\":%zu"
          "}\n",
          scale_bits,
          hex_encode(ptr, comp_size).c_str(),
          comp_size);
}

// ---------------------------------------------------------------------------
// Byte interleaved2 encode (division path)
// ---------------------------------------------------------------------------
// enc-stream-byte-interleaved2-div: division-based interleaved encode
static void trace_enc_stream_byte_interleaved2_div(uint32_t scale_bits,
                                                   const std::vector<uint32_t>& freqs,
                                                   const std::vector<uint8_t>& input)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   uint8_t buf[64 * 1024];
   uint8_t* ptr = buf + sizeof(buf);
   RansState state0, state1;
   RansEncInit(&state0);
   RansEncInit(&state1);
   int count = (int)input.size();
   if (count & 1) {
       int s = input[count - 1];
       RansEncPut(&state0, &ptr, cum_freqs[s], freqs[s], scale_bits);
   }
   for (int i = count & ~1; i > 0; i -= 2) {
       int s1 = input[i - 1];
       int s0 = input[i - 2];
       RansEncPut(&state1, &ptr, cum_freqs[s1], freqs[s1], scale_bits);
       RansEncPut(&state0, &ptr, cum_freqs[s0], freqs[s0], scale_bits);
   }
   RansEncFlush(&state1, &ptr);
   RansEncFlush(&state0, &ptr);
   size_t comp_size = sizeof(buf) - (ptr - buf);
   printf("{\"op\":\"enc-stream-byte-interleaved2-div\""
          ",\"scale_bits\":%u"
          ",\"compressed_hex\":\"%s\""
          ",\"compressed_size\":%zu"
          "}\n",
          scale_bits,
          hex_encode(ptr, comp_size).c_str(),
          comp_size);
}

// ---------------------------------------------------------------------------
// Byte interleaved2 decode helper (shared by div and reciprocal)
// ---------------------------------------------------------------------------
static void trace_dec_stream_byte_interleaved2_impl(uint32_t scale_bits,
                                                     const std::vector<uint32_t>& freqs,
                                                     const std::vector<uint8_t>& compressed,
                                                     size_t num_symbols,
                                                     const char* op_name,
                                                     bool use_symbol)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   // Build decoder symbols for reciprocal path
   RansDecSymbol dsyms[256];
   for (int i = 0; i < 256; i++) {
       if (freqs[i] > 0) {
           RansDecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
       }
   }

   // Init two interleaved states from the compressed stream
   std::vector<uint8_t> buf(compressed.begin(), compressed.end());
   uint8_t* ptr = buf.data();
   RansState state0, state1;
   RansDecInit(&state0, &ptr);
   RansDecInit(&state1, &ptr);

   size_t n = num_symbols;
   size_t even_n = n & ~(size_t)1;
   std::vector<uint8_t> output(n);

   // Decode pairs: step both states first, then renorm both
   // Prevents one state consuming the other's renorm bytes
   for (size_t pos = 0; pos < even_n; pos += 2) {
       uint32_t cf0 = RansDecGet(&state0, scale_bits);
       uint32_t cf1 = RansDecGet(&state1, scale_bits);
       // Find symbols by cumulative frequency
       int s0 = 0, s1 = 0;
       for (int j = 0; j < 256; j++) {
           if (cf0 >= cum_freqs[j] && cf0 < cum_freqs[j+1]) { s0 = j; break; }
       }
       for (int j = 0; j < 256; j++) {
           if (cf1 >= cum_freqs[j] && cf1 < cum_freqs[j+1]) { s1 = j; break; }
       }
       output[pos] = (uint8_t)s0;
       output[pos + 1] = (uint8_t)s1;

       if (use_symbol) {
           RansDecAdvanceSymbolStep(&state0, &dsyms[s0], scale_bits);
           RansDecAdvanceSymbolStep(&state1, &dsyms[s1], scale_bits);
       } else {
           RansDecAdvanceStep(&state0, cum_freqs[s0], freqs[s0], scale_bits);
           RansDecAdvanceStep(&state1, cum_freqs[s1], freqs[s1], scale_bits);
       }
       RansDecRenorm(&state0, &ptr);
       RansDecRenorm(&state1, &ptr);
   }

   // Odd tail: decode last symbol from state0
   if (even_n < n) {
       uint32_t cf = RansDecGet(&state0, scale_bits);
       int s = 0;
       for (int j = 0; j < 256; j++) {
           if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
       }
       output[n - 1] = (uint8_t)s;
       if (use_symbol) {
           RansDecAdvanceSymbol(&state0, &ptr, &dsyms[s], scale_bits);
       } else {
           RansDecAdvance(&state0, &ptr, cum_freqs[s], freqs[s], scale_bits);
       }
   }

   printf("{\"op\":\"%s\""
          ",\"scale_bits\":%u"
          ",\"num_symbols\":%zu"
          ",\"decoded_hex\":\"%s\""
          "}\n",
          op_name, scale_bits, n,
          hex_encode(output.data(), output.size()).c_str());
}

static void trace_dec_stream_byte_interleaved2(uint32_t scale_bits,
                                                const std::vector<uint32_t>& freqs,
                                                const std::vector<uint8_t>& compressed,
                                                size_t num_symbols)
{
    trace_dec_stream_byte_interleaved2_impl(scale_bits, freqs, compressed, num_symbols,
                                             "dec-stream-byte-interleaved2", true);
}

static void trace_dec_stream_byte_interleaved2_div(uint32_t scale_bits,
                                                    const std::vector<uint32_t>& freqs,
                                                    const std::vector<uint8_t>& compressed,
                                                    size_t num_symbols)
{
    trace_dec_stream_byte_interleaved2_impl(scale_bits, freqs, compressed, num_symbols,
                                             "dec-stream-byte-interleaved2-div", false);
}

// ---------------------------------------------------------------------------
// 64-bit interleaved2 encode (reciprocal fast path)
// ---------------------------------------------------------------------------
static void trace_enc_stream_r64_interleaved2(uint32_t scale_bits,
                                              const std::vector<uint32_t>& freqs,
                                              const std::vector<uint8_t>& input)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   Rans64EncSymbol esyms[256];
   for (int i = 0; i < 256; i++) {
       if (freqs[i] > 0) {
           Rans64EncSymbolInit(&esyms[i], cum_freqs[i], freqs[i], scale_bits);
       }
   }
   uint32_t buf[64 * 1024];
   uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
   Rans64State state0, state1;
   Rans64EncInit(&state0);
   Rans64EncInit(&state1);
   int count = (int)input.size();
   if (count & 1) {
       int s = input[count - 1];
       Rans64EncPutSymbol(&state0, &ptr, &esyms[s], scale_bits);
   }
   for (int i = count & ~1; i > 0; i -= 2) {
       int s1 = input[i - 1];
       int s0 = input[i - 2];
       Rans64EncPutSymbol(&state1, &ptr, &esyms[s1], scale_bits);
       Rans64EncPutSymbol(&state0, &ptr, &esyms[s0], scale_bits);
   }
   Rans64EncFlush(&state1, &ptr);
   Rans64EncFlush(&state0, &ptr);
   size_t comp_words = (size_t)((buf + sizeof(buf) / sizeof(buf[0])) - ptr);
   size_t comp_size = comp_words * sizeof(uint32_t);
   printf("{\"op\":\"enc-stream-r64-interleaved2\""
          ",\"scale_bits\":%u"
          ",\"compressed_hex\":\"%s\""
          ",\"compressed_size\":%zu"
          "}\n",
          scale_bits,
          hex_encode((const uint8_t*)ptr, comp_size).c_str(),
          comp_size);
}

// ---------------------------------------------------------------------------
// 64-bit interleaved2 encode (division path)
// ---------------------------------------------------------------------------
static void trace_enc_stream_r64_interleaved2_div(uint32_t scale_bits,
                                                   const std::vector<uint32_t>& freqs,
                                                   const std::vector<uint8_t>& input)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   uint32_t buf[64 * 1024];
   uint32_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
   Rans64State state0, state1;
   Rans64EncInit(&state0);
   Rans64EncInit(&state1);
   int count = (int)input.size();
   if (count & 1) {
       int s = input[count - 1];
       Rans64EncPut(&state0, &ptr, cum_freqs[s], freqs[s], scale_bits);
   }
   for (int i = count & ~1; i > 0; i -= 2) {
       int s1 = input[i - 1];
       int s0 = input[i - 2];
       Rans64EncPut(&state1, &ptr, cum_freqs[s1], freqs[s1], scale_bits);
       Rans64EncPut(&state0, &ptr, cum_freqs[s0], freqs[s0], scale_bits);
   }
   Rans64EncFlush(&state1, &ptr);
   Rans64EncFlush(&state0, &ptr);
   size_t comp_words = (size_t)((buf + sizeof(buf) / sizeof(buf[0])) - ptr);
   size_t comp_size = comp_words * sizeof(uint32_t);
   printf("{\"op\":\"enc-stream-r64-interleaved2-div\""
          ",\"scale_bits\":%u"
          ",\"compressed_hex\":\"%s\""
          ",\"compressed_size\":%zu"
          "}\n",
          scale_bits,
          hex_encode((const uint8_t*)ptr, comp_size).c_str(),
          comp_size);
}

// ---------------------------------------------------------------------------
// 64-bit interleaved2 decode helper (shared by div and reciprocal)
// ---------------------------------------------------------------------------
static void trace_dec_stream_r64_interleaved2_impl(uint32_t scale_bits,
                                                    const std::vector<uint32_t>& freqs,
                                                    const std::vector<uint8_t>& compressed,
                                                    size_t num_symbols,
                                                    const char* op_name,
                                                    bool use_symbol)
{
   uint32_t cum_freqs[257];
   cum_freqs[0] = 0;
   for (int i = 0; i < 256; i++) {
       cum_freqs[i+1] = cum_freqs[i] + freqs[i];
   }
   Rans64DecSymbol dsyms[256];
   for (int i = 0; i < 256; i++) {
       if (freqs[i] > 0) {
           Rans64DecSymbolInit(&dsyms[i], cum_freqs[i], freqs[i]);
       }
   }

   // Init two interleaved states from the compressed stream (as word array)
   std::vector<uint32_t> words(compressed.size() / 4 + 2, 0);
   memcpy(words.data(), compressed.data(), compressed.size());
   uint32_t* ptr = words.data();
   Rans64State state0, state1;
   Rans64DecInit(&state0, &ptr);
   Rans64DecInit(&state1, &ptr);

   size_t n = num_symbols;
   size_t even_n = n & ~(size_t)1;
   std::vector<uint8_t> output(n);

   for (size_t pos = 0; pos < even_n; pos += 2) {
       uint32_t cf0 = Rans64DecGet(&state0, scale_bits);
       uint32_t cf1 = Rans64DecGet(&state1, scale_bits);
       int s0 = 0, s1 = 0;
       for (int j = 0; j < 256; j++) {
           if (cf0 >= cum_freqs[j] && cf0 < cum_freqs[j+1]) { s0 = j; break; }
       }
       for (int j = 0; j < 256; j++) {
           if (cf1 >= cum_freqs[j] && cf1 < cum_freqs[j+1]) { s1 = j; break; }
       }
       output[pos] = (uint8_t)s0;
       output[pos + 1] = (uint8_t)s1;

       if (use_symbol) {
           Rans64DecAdvanceSymbolStep(&state0, &dsyms[s0], scale_bits);
           Rans64DecAdvanceSymbolStep(&state1, &dsyms[s1], scale_bits);
       } else {
           Rans64DecAdvanceStep(&state0, cum_freqs[s0], freqs[s0], scale_bits);
           Rans64DecAdvanceStep(&state1, cum_freqs[s1], freqs[s1], scale_bits);
       }
       Rans64DecRenorm(&state0, &ptr);
       Rans64DecRenorm(&state1, &ptr);
   }

   if (even_n < n) {
       uint32_t cf = Rans64DecGet(&state0, scale_bits);
       int s = 0;
       for (int j = 0; j < 256; j++) {
           if (cf >= cum_freqs[j] && cf < cum_freqs[j+1]) { s = j; break; }
       }
       output[n - 1] = (uint8_t)s;
       if (use_symbol) {
           Rans64DecAdvanceSymbol(&state0, &ptr, &dsyms[s], scale_bits);
       } else {
           Rans64DecAdvance(&state0, &ptr, cum_freqs[s], freqs[s], scale_bits);
       }
   }

   printf("{\"op\":\"%s\""
          ",\"scale_bits\":%u"
          ",\"num_symbols\":%zu"
          ",\"decoded_hex\":\"%s\""
          "}\n",
          op_name, scale_bits, n,
          hex_encode(output.data(), output.size()).c_str());
}

static void trace_dec_stream_r64_interleaved2(uint32_t scale_bits,
                                               const std::vector<uint32_t>& freqs,
                                               const std::vector<uint8_t>& compressed,
                                               size_t num_symbols)
{
    trace_dec_stream_r64_interleaved2_impl(scale_bits, freqs, compressed, num_symbols,
                                            "dec-stream-r64-interleaved2", true);
}

static void trace_dec_stream_r64_interleaved2_div(uint32_t scale_bits,
                                                   const std::vector<uint32_t>& freqs,
                                                   const std::vector<uint8_t>& compressed,
                                                   size_t num_symbols)
{
    trace_dec_stream_r64_interleaved2_impl(scale_bits, freqs, compressed, num_symbols,
                                            "dec-stream-r64-interleaved2-div", false);
}

// ===========================================================================
// Word rANS stream operations (rans_word_sse41.h scalar path)
// ===========================================================================

// Helper: build word rANS tables from a frequency model
static void build_word_tables(RansWordTables* tab, const std::vector<uint32_t>& freqs, uint32_t scale_bits)
{
    uint32_t cum = 0;
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansWordTablesInitSymbol(tab, (uint8_t)i, cum, freqs[i]);
            cum += freqs[i];
        }
    }
}

// enc-stream-word: word rANS encode with self-decode
static void trace_enc_stream_word(uint32_t scale_bits,
                                  const std::vector<uint32_t>& freqs,
                                  const std::vector<uint8_t>& input)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    // Build word decoder tables for self-decode
    RansWordTables tab;
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansWordTablesInitSymbol(&tab, (uint8_t)i, cum_freqs[i], freqs[i]);
        }
    }

    uint16_t buf[64 * 1024];
    uint16_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
    RansWordEnc state = RansWordEncInit();

    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i-1];
        RansWordEncPut(&state, &ptr, cum_freqs[s], freqs[s]);
    }
    RansWordEncFlush(&state, &ptr);

    size_t comp_words = (size_t)((buf + sizeof(buf) / sizeof(buf[0])) - ptr);
    size_t comp_bytes = comp_words * sizeof(uint16_t);
    std::string comp_hex = hex_encode((const uint8_t*)ptr, comp_bytes);

    // Self-decode to verify
    std::vector<uint16_t> words(comp_words + 2, 0);
    memcpy(words.data(), ptr, comp_bytes);
    uint16_t* dec_ptr = words.data();
    RansWordDec dec_state;
    RansWordDecInit(&dec_state, &dec_ptr);
    bool decode_ok = true;
    for (size_t i = 0; i < input.size(); i++) {
        uint8_t s = RansWordDecSym(&dec_state, &tab);
        RansWordDecRenorm(&dec_state, &dec_ptr);
        if (s != input[i]) { decode_ok = false; break; }
    }

    printf("{\"op\":\"enc-stream-word\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"compressed_size\":%zu"
           ",\"decode_ok\":%s"
           "}\n",
           scale_bits, input.size(), comp_hex.c_str(), comp_bytes,
           decode_ok ? "true" : "false");
}

// dec-stream-word: word rANS decode (table-based)
static void trace_dec_stream_word(uint32_t scale_bits,
                                  const std::vector<uint32_t>& freqs,
                                  const std::vector<uint8_t>& compressed,
                                  size_t num_symbols)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    // Build word tables
    RansWordTables tab;
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansWordTablesInitSymbol(&tab, (uint8_t)i, cum_freqs[i], freqs[i]);
        }
    }

    // Decode: read u16 words from the compressed stream
    std::vector<uint16_t> words(compressed.size() / 2 + 2, 0);
    memcpy(words.data(), compressed.data(), compressed.size());
    uint16_t* ptr = words.data();

    RansWordDec state;
    RansWordDecInit(&state, &ptr);

    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        uint8_t s = RansWordDecSym(&state, &tab);
        RansWordDecRenorm(&state, &ptr);
        output[i] = s;
    }

    printf("{\"op\":\"dec-stream-word\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           "}\n",
           scale_bits, num_symbols,
           hex_encode(output.data(), output.size()).c_str());
}

// enc-stream-word-interleaved2: two-state interleaved word rANS encode
static void trace_enc_stream_word_interleaved2(uint32_t scale_bits,
                                               const std::vector<uint32_t>& freqs,
                                               const std::vector<uint8_t>& input)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    RansWordTables tab;
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansWordTablesInitSymbol(&tab, (uint8_t)i, cum_freqs[i], freqs[i]);
        }
    }

    uint16_t buf[64 * 1024];
    uint16_t* ptr = buf + sizeof(buf) / sizeof(buf[0]);
    RansWordEnc state0, state1;
    state0 = RansWordEncInit();
    state1 = RansWordEncInit();
    int count = (int)input.size();

    // Two-state interleaved: same pair pattern as byte/R64
    // odd tail to state0, pairs (i-1→state1, i-2→state0) in reverse
    if (count & 1) {
        int s = input[count - 1];
        RansWordEncPut(&state0, &ptr, cum_freqs[s], freqs[s]);
    }
    for (int i = count & ~1; i > 0; i -= 2) {
        int s1 = input[i - 1];
        int s0 = input[i - 2];
        RansWordEncPut(&state1, &ptr, cum_freqs[s1], freqs[s1]);
        RansWordEncPut(&state0, &ptr, cum_freqs[s0], freqs[s0]);
    }
    RansWordEncFlush(&state1, &ptr);
    RansWordEncFlush(&state0, &ptr);

    size_t comp_words = (size_t)((buf + sizeof(buf) / sizeof(buf[0])) - ptr);
    size_t comp_bytes = comp_words * sizeof(uint16_t);

    printf("{\"op\":\"enc-stream-word-interleaved2\""
           ",\"scale_bits\":%u"
           ",\"compressed_hex\":\"%s\""
           ",\"compressed_size\":%zu"
           "}\n",
           scale_bits,
           hex_encode((const uint8_t*)ptr, comp_bytes).c_str(),
           comp_bytes);
}

// dec-stream-word-interleaved2: two-state interleaved word rANS decode
static void trace_dec_stream_word_interleaved2(uint32_t scale_bits,
                                               const std::vector<uint32_t>& freqs,
                                               const std::vector<uint8_t>& compressed,
                                               size_t num_symbols)
{
    uint32_t cum_freqs[257];
    cum_freqs[0] = 0;
    for (int i = 0; i < 256; i++) {
        cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    RansWordTables tab;
    for (int i = 0; i < 256; i++) {
        if (freqs[i] > 0) {
            RansWordTablesInitSymbol(&tab, (uint8_t)i, cum_freqs[i], freqs[i]);
        }
    }

    // Decode: read u16 words, init two interleaved states
    std::vector<uint16_t> words(compressed.size() / 2 + 4, 0);
    memcpy(words.data(), compressed.data(), compressed.size());
    uint16_t* ptr = words.data();

    RansWordDec state0, state1;
    RansWordDecInit(&state0, &ptr);
    RansWordDecInit(&state1, &ptr);

    size_t n = num_symbols;
    size_t even_n = n & ~(size_t)1;
    std::vector<uint8_t> output(n);

    // Decode pairs: sym both states, then renorm both
    for (size_t pos = 0; pos < even_n; pos += 2) {
        output[pos] = RansWordDecSym(&state0, &tab);
        output[pos + 1] = RansWordDecSym(&state1, &tab);
        RansWordDecRenorm(&state0, &ptr);
        RansWordDecRenorm(&state1, &ptr);
    }

    // Odd tail from state0
    if (even_n < n) {
        output[n - 1] = RansWordDecSym(&state0, &tab);
        RansWordDecRenorm(&state0, &ptr);
    }

    printf("{\"op\":\"dec-stream-word-interleaved2\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           "}\n",
           scale_bits, n,
           hex_encode(output.data(), output.size()).c_str());
}

// ===========================================================================
// Forward declarations for alias operations (defined after main)
// ===========================================================================
static void trace_alias_table(uint32_t scale_bits, const std::vector<uint32_t>& freqs_in);
static void trace_enc_stream_alias(uint32_t scale_bits, const std::vector<uint32_t>& freqs_in, const std::vector<uint8_t>& input);
static void trace_dec_stream_alias(uint32_t scale_bits, const std::vector<uint32_t>& freqs_in, const std::vector<uint8_t>& compressed, size_t num_symbols);
static void trace_enc_stream_alias_interleaved2(uint32_t scale_bits, const std::vector<uint32_t>& freqs_in, const std::vector<uint8_t>& input);
static void trace_dec_stream_alias_interleaved2(uint32_t scale_bits, const std::vector<uint32_t>& freqs_in, const std::vector<uint8_t>& compressed, size_t num_symbols);

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
        if (argc != 6) usage(argv[0]);
        trace_r64_enc_put_symbol(parse_u64(argv[2]),
                                 parse_u32(argv[3]),
                                 parse_u32(argv[4]),
                                 parse_u32(argv[5]));
    } else if (strcmp(op, "r64-mul-hi") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_r64_mul_hi(parse_u64(argv[2]),
                         parse_u64(argv[3]));
    } else if (strcmp(op, "r64-dec-renorm") == 0) {
        if (argc != 3) usage(argv[0]);
        trace_r64_dec_renorm(parse_u64(argv[2]));

    // ---- Stream operations ----
    } else if (strcmp(op, "enc-stream-byte-div") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_stream_byte_div(parse_u32(argv[2]),
                                  parse_freq_csv(argv[3]),
                                  hex_decode(argv[4]));
    } else if (strcmp(op, "enc-stream-r64-div") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_stream_r64_div(parse_u32(argv[2]),
                                 parse_freq_csv(argv[3]),
                                 hex_decode(argv[4]));
    } else if (strcmp(op, "dec-stream-byte-div") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_dec_stream_byte_div(parse_u32(argv[2]),
                                  parse_freq_csv(argv[3]),
                                  hex_decode(argv[4]),
                                  parse_u32(argv[5]));
    } else if (strcmp(op, "dec-stream-r64-div") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_dec_stream_r64_div(parse_u32(argv[2]),
                                 parse_freq_csv(argv[3]),
                                 hex_decode(argv[4]),
                                 parse_u32(argv[5]));
    } else if (strcmp(op, "enc-stream-byte") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_byte(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-byte") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_byte(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "enc-stream-r64") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_r64(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-r64") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_r64(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "enc-stream-byte-interleaved2") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_byte_interleaved2(scale_bits, freqs, input);
    } else if (strcmp(op, "enc-stream-byte-interleaved2-div") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_byte_interleaved2_div(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-byte-interleaved2") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_byte_interleaved2(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "dec-stream-byte-interleaved2-div") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_byte_interleaved2_div(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "enc-stream-r64-interleaved2") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_r64_interleaved2(scale_bits, freqs, input);
    } else if (strcmp(op, "enc-stream-r64-interleaved2-div") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_r64_interleaved2_div(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-r64-interleaved2") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_r64_interleaved2(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "dec-stream-r64-interleaved2-div") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_r64_interleaved2_div(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "enc-stream-word") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_word(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-word") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_word(scale_bits, freqs, compressed, num_symbols);
    } else if (strcmp(op, "enc-stream-word-interleaved2") == 0) {
        if (argc != 5) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> input = hex_decode(argv[4]);
        trace_enc_stream_word_interleaved2(scale_bits, freqs, input);
    } else if (strcmp(op, "dec-stream-word-interleaved2") == 0) {
        if (argc != 6) usage(argv[0]);
        uint32_t scale_bits = parse_u32(argv[2]);
        std::vector<uint32_t> freqs = parse_freq_csv(argv[3]);
        std::vector<uint8_t> compressed = hex_decode(argv[4]);
        size_t num_symbols = parse_u32(argv[5]);
        trace_dec_stream_word_interleaved2(scale_bits, freqs, compressed, num_symbols);
    // ---- Alias operations ----
    } else if (strcmp(op, "trace-alias-table") == 0) {
        if (argc != 4) usage(argv[0]);
        trace_alias_table(parse_u32(argv[2]), parse_freq_csv(argv[3]));
    } else if (strcmp(op, "enc-stream-alias") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_stream_alias(parse_u32(argv[2]), parse_freq_csv(argv[3]), hex_decode(argv[4]));
    } else if (strcmp(op, "dec-stream-alias") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_dec_stream_alias(parse_u32(argv[2]), parse_freq_csv(argv[3]), hex_decode(argv[4]), parse_u32(argv[5]));
    } else if (strcmp(op, "enc-stream-alias-interleaved2") == 0) {
        if (argc != 5) usage(argv[0]);
        trace_enc_stream_alias_interleaved2(parse_u32(argv[2]), parse_freq_csv(argv[3]), hex_decode(argv[4]));
    } else if (strcmp(op, "dec-stream-alias-interleaved2") == 0) {
        if (argc != 6) usage(argv[0]);
        trace_dec_stream_alias_interleaved2(parse_u32(argv[2]), parse_freq_csv(argv[3]), hex_decode(argv[4]), parse_u32(argv[5]));
    } else {
        fprintf(stderr, "Unknown operation: %s\n", op);
        usage(argv[0]);
    }

    return 0;
}

// ===========================================================================
// Alias method operations
// ===========================================================================
// The alias method is an alternative rANS coding path that uses a precomputed
// alias table to avoid symbol search during decoding.  It divides the
// probability space into 256 equal-sized buckets, each holding at most 2
// symbols.  Encoding and decoding use direct table lookups instead of
// cumulative-frequency searches.
//
// Adapted from upstream ryg rANS main_alias.cpp.
// ===========================================================================

static const int ALIAS_LOG2NSYMS = 8;
static const int ALIAS_NSYMS = 1 << ALIAS_LOG2NSYMS;

// Alias-specific state built from a frequency model.
struct AliasStats {
    uint32_t freqs[ALIAS_NSYMS];
    uint32_t cum_freqs[ALIAS_NSYMS + 1];
    uint32_t divider[ALIAS_NSYMS];
    uint32_t slot_adjust[ALIAS_NSYMS * 2];
    uint32_t slot_freqs[ALIAS_NSYMS * 2];
    uint8_t  sym_id[ALIAS_NSYMS * 2];
    uint32_t* alias_remap;  // heap-allocated, size = 1 << scale_bits

    void normalize_freqs(uint32_t target_total) {
        calc_cum_freqs();
        uint32_t cur_total = cum_freqs[ALIAS_NSYMS];
        if (cur_total == 0) {
            for (int i = 0; i < ALIAS_NSYMS; i++) freqs[i] = 1;
            cur_total = ALIAS_NSYMS;
        }

        // Resample distribution based on cumulative freqs
        for (int i = 1; i <= ALIAS_NSYMS; i++) {
            cum_freqs[i] = (uint32_t)(((uint64_t)target_total * cum_freqs[i]) / cur_total);
        }

        // Zero-frequency theft: if a non-zero symbol was nuked to zero, steal from the
        // smallest frequency > 1.
        for (int i = 0; i < ALIAS_NSYMS; i++) {
            if (freqs[i] && cum_freqs[i+1] == cum_freqs[i]) {
                uint32_t best_freq = ~0u;
                int best_steal = -1;
                for (int j = 0; j < ALIAS_NSYMS; j++) {
                    uint32_t f = cum_freqs[j+1] - cum_freqs[j];
                    if (f > 1 && f < best_freq) {
                        best_freq = f;
                        best_steal = j;
                    }
                }
                if (best_steal < i) {
                    for (int j = best_steal + 1; j <= i; j++)
                        cum_freqs[j]--;
                } else {
                    for (int j = i + 1; j <= best_steal; j++)
                        cum_freqs[j]++;
                }
            }
        }

        // Recompute freqs from cum
        for (int i = 0; i < ALIAS_NSYMS; i++) {
            freqs[i] = cum_freqs[i+1] - cum_freqs[i];
        }
    }

    void calc_cum_freqs() {
        cum_freqs[0] = 0;
        for (int i = 0; i < ALIAS_NSYMS; i++)
            cum_freqs[i+1] = cum_freqs[i] + freqs[i];
    }

    void make_alias_table(uint32_t scale_bits) {
        uint32_t total = 1u << scale_bits;
        uint32_t tgt_sum = total / ALIAS_NSYMS;

        delete[] alias_remap;
        alias_remap = new uint32_t[total];

        // Phase 1: Vose's alias construction
        uint32_t remaining[ALIAS_NSYMS];
        for (int i = 0; i < ALIAS_NSYMS; i++) {
            remaining[i] = freqs[i];
            divider[i] = tgt_sum;
            sym_id[i*2 + 0] = (uint8_t)i;
            sym_id[i*2 + 1] = (uint8_t)i;
        }

        int cur_large = 0;
        while (cur_large < ALIAS_NSYMS && remaining[cur_large] < tgt_sum)
            cur_large++;
        int cur_small = 0;
        while (cur_small < ALIAS_NSYMS && remaining[cur_small] >= tgt_sum)
            cur_small++;
        int next_small = cur_small + 1;

        while (cur_large < ALIAS_NSYMS && cur_small < ALIAS_NSYMS) {
            sym_id[cur_small * 2] = (uint8_t)cur_large;
            divider[cur_small] = remaining[cur_small];
            remaining[cur_large] -= tgt_sum - divider[cur_small];

            if (remaining[cur_large] >= tgt_sum || next_small <= cur_large) {
                cur_small = next_small;
                while (cur_small < ALIAS_NSYMS && remaining[cur_small] >= tgt_sum)
                    cur_small++;
                next_small = cur_small + 1;
            } else {
                cur_small = cur_large;
            }

            while (cur_large < ALIAS_NSYMS && remaining[cur_large] < tgt_sum)
                cur_large++;
        }

        // Phase 2: Distribute code slots
        uint32_t assigned[ALIAS_NSYMS] = { 0 };

        for (int i = 0; i < ALIAS_NSYMS; i++) {
            int j = sym_id[i*2 + 0];
            uint32_t sym0_height = divider[i];
            uint32_t sym1_height = tgt_sum - divider[i];
            uint32_t base0 = assigned[i];
            uint32_t base1 = assigned[j];
            uint32_t cbase0 = cum_freqs[i] + base0;
            uint32_t cbase1 = cum_freqs[j] + base1;

            divider[i] = (uint32_t)i * tgt_sum + sym0_height;

            slot_freqs[i*2 + 1] = freqs[i];
            slot_freqs[i*2 + 0] = freqs[j];
            slot_adjust[i*2 + 1] = (uint32_t)i * tgt_sum - base0;
            slot_adjust[i*2 + 0] = (uint32_t)i * tgt_sum - (base1 - sym0_height);

            for (uint32_t k = 0; k < sym0_height; k++) {
                if (cbase0 + k < total)
                    alias_remap[cbase0 + k] = k + (uint32_t)i * tgt_sum;
            }
            for (uint32_t k = 0; k < sym1_height; k++) {
                if (cbase1 + k < total)
                    alias_remap[cbase1 + k] = (k + sym0_height) + (uint32_t)i * tgt_sum;
            }

            assigned[i] += sym0_height;
            assigned[j] += sym1_height;
        }
    }

    AliasStats() : alias_remap(nullptr) {}
    ~AliasStats() { if (alias_remap) delete[] alias_remap; }
};

// Alias encode: same renormalization as standard byte rANS, then use alias_remap
// instead of adding the cumulative-frequency offset.
static inline void RansEncPutAlias(RansState* r, uint8_t** pptr,
                                   uint32_t freq, uint32_t cum,
                                   uint32_t alias_val, uint32_t scale_bits)
{
    RansState x = RansEncRenorm(*r, pptr, freq, scale_bits);
    *r = ((x / freq) << scale_bits) + alias_val;
}

// Alias decode: extract bottom bits, find bucket/slot, compute new state,
// then renormalize. Returns the decoded symbol.
//
// Slot assignment per the upstream main_alias.cpp convention:
//   slot 2*b   = primary symbol (used when xm <  divider[b])
//   slot 2*b+1 = alias symbol   (used when xm >= divider[b])
static inline uint8_t RansDecGetAlias(RansState* r, uint8_t** pptr,
                                      const AliasStats* syms,
                                      uint32_t scale_bits)
{
    uint32_t mask = (1u << scale_bits) - 1;
    uint32_t x = *r;
    uint32_t xm = x & mask;
    uint32_t bucket_id = xm >> (scale_bits - ALIAS_LOG2NSYMS);

    // Decide which slot: primary (2*b) if xm < divider, alias (2*b+1) otherwise.
    // divider[b] stores the absolute position of the split: bucket_start + primary_amount.
    uint32_t bucket2 = bucket_id * 2;
    if (xm < syms->divider[bucket_id]) bucket2++;

    // Compute new state: freq * (x >> scale_bits) + xm - adjust
    uint32_t new_x = syms->slot_freqs[bucket2] * (x >> scale_bits) + xm - syms->slot_adjust[bucket2];

    // Renorm
    if (new_x < RANS_BYTE_L) {
        uint8_t* ptr = *pptr;
        do new_x = (new_x << 8) | *ptr++; while (new_x < RANS_BYTE_L);
        *pptr = ptr;
    }

    *r = new_x;
    return syms->sym_id[bucket2];
}

// ---------------------------------------------------------------------------
// trace-alias-table: output the alias table as JSON
// ---------------------------------------------------------------------------
static void trace_alias_table(uint32_t scale_bits,
                              const std::vector<uint32_t>& freqs_in)
{
    uint32_t total = 1u << scale_bits;
    uint32_t B = total / ALIAS_NSYMS;

    AliasStats syms;
    for (int i = 0; i < ALIAS_NSYMS && i < (int)freqs_in.size(); i++) {
        syms.freqs[i] = freqs_in[i];
    }
    for (int i = (int)freqs_in.size(); i < ALIAS_NSYMS; i++) {
        syms.freqs[i] = 0;
    }
    syms.normalize_freqs(total);
    syms.make_alias_table(scale_bits);

    printf("{\"op\":\"trace-alias-table\""
           ",\"scale_bits\":%u"
           ",\"total\":%u"
           ",\"bucket_size\":%u"
           ",\"freqs\":[",
           scale_bits, total, B);
    for (int i = 0; i < ALIAS_NSYMS; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.freqs[i]);
    }
    printf("]"
           ",\"cum_freqs\":[");
    for (int i = 0; i <= ALIAS_NSYMS; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.cum_freqs[i]);
    }
    printf("]"
           ",\"divider\":[");
    for (int i = 0; i < ALIAS_NSYMS; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.divider[i]);
    }
    printf("]"
           ",\"slot_freqs\":[");
    for (int i = 0; i < ALIAS_NSYMS * 2; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.slot_freqs[i]);
    }
    printf("]"
           ",\"slot_adjust\":[");
    for (int i = 0; i < ALIAS_NSYMS * 2; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.slot_adjust[i]);
    }
    printf("]"
           ",\"sym_id\":[");
    for (int i = 0; i < ALIAS_NSYMS * 2; i++) {
        if (i > 0) printf(",");
        printf("%u", (unsigned)syms.sym_id[i]);
    }
    printf("]"
           ",\"alias_remap\":[");
    for (uint32_t i = 0; i < total; i++) {
        if (i > 0) printf(",");
        printf("%u", syms.alias_remap[i]);
    }
    printf("]"
           "}\n");
}

// ---------------------------------------------------------------------------
// enc-stream-alias: encode using alias method, self-decode to verify
// ---------------------------------------------------------------------------
static void trace_enc_stream_alias(uint32_t scale_bits,
                                   const std::vector<uint32_t>& freqs_in,
                                   const std::vector<uint8_t>& input)
{
    uint32_t total = 1u << scale_bits;

    AliasStats syms;
    for (int i = 0; i < ALIAS_NSYMS && i < (int)freqs_in.size(); i++) {
        syms.freqs[i] = freqs_in[i];
    }
    for (int i = (int)freqs_in.size(); i < ALIAS_NSYMS; i++) {
        syms.freqs[i] = 0;
    }
    syms.normalize_freqs(total);
    syms.make_alias_table(scale_bits);

    // Encode (reverse scan)
    uint8_t buf[64 * 1024];
    uint8_t* ptr = buf + sizeof(buf);
    RansState state;
    RansEncInit(&state);

    for (size_t i = input.size(); i > 0; i--) {
        int s = input[i - 1];
        uint32_t freq = syms.freqs[s];
        uint32_t cum = syms.cum_freqs[s];
        // remainder after renormalization
        RansState x = RansEncRenorm(state, &ptr, freq, scale_bits);
        uint32_t rem = x % freq;
        uint32_t offset = rem + cum;
        state = ((x / freq) << scale_bits) + syms.alias_remap[offset];
    }
    RansEncFlush(&state, &ptr);

    size_t comp_size = sizeof(buf) - (ptr - buf);
    std::string comp_hex = hex_encode(ptr, comp_size);

    // Self-decode to verify
    uint8_t* dec_ptr = ptr;
    RansState dec_state;
    RansDecInit(&dec_state, &dec_ptr);

    bool decode_ok = true;
    for (size_t i = 0; i < input.size(); i++) {
        uint8_t s = RansDecGetAlias(&dec_state, &dec_ptr, &syms, scale_bits);
        if (s != input[i]) { decode_ok = false; break; }
    }

    printf("{\"op\":\"enc-stream-alias\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_hex\":\"%s\""
           ",\"decode_ok\":%s"
           "}\n",
           scale_bits, input.size(), comp_hex.c_str(),
           decode_ok ? "true" : "false");
}

// ---------------------------------------------------------------------------
// dec-stream-alias: decode an alias-encoded stream
// ---------------------------------------------------------------------------
static void trace_dec_stream_alias(uint32_t scale_bits,
                                   const std::vector<uint32_t>& freqs_in,
                                   const std::vector<uint8_t>& compressed,
                                   size_t num_symbols)
{
    uint32_t total = 1u << scale_bits;

    AliasStats syms;
    for (int i = 0; i < ALIAS_NSYMS && i < (int)freqs_in.size(); i++) {
        syms.freqs[i] = freqs_in[i];
    }
    for (int i = (int)freqs_in.size(); i < ALIAS_NSYMS; i++) {
        syms.freqs[i] = 0;
    }
    syms.normalize_freqs(total);
    syms.make_alias_table(scale_bits);

    std::vector<uint8_t> buf(compressed.begin(), compressed.end());
    uint8_t* ptr = buf.data();

    RansState state;
    RansDecInit(&state, &ptr);

    std::vector<uint8_t> output(num_symbols);
    for (size_t i = 0; i < num_symbols; i++) {
        output[i] = RansDecGetAlias(&state, &ptr, &syms, scale_bits);
    }

    printf("{\"op\":\"dec-stream-alias\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           "}\n",
           scale_bits, num_symbols,
           hex_encode(output.data(), output.size()).c_str());
}

// ---------------------------------------------------------------------------
// Alias interleaved2 encode
// ---------------------------------------------------------------------------
static void trace_enc_stream_alias_interleaved2(uint32_t scale_bits,
                                                const std::vector<uint32_t>& freqs_in,
                                                const std::vector<uint8_t>& input)
{
    uint32_t total = 1u << scale_bits;

    AliasStats syms;
    for (int i = 0; i < ALIAS_NSYMS && i < (int)freqs_in.size(); i++) {
        syms.freqs[i] = freqs_in[i];
    }
    for (int i = (int)freqs_in.size(); i < ALIAS_NSYMS; i++) {
        syms.freqs[i] = 0;
    }
    syms.normalize_freqs(total);
    syms.make_alias_table(scale_bits);

    uint8_t buf[64 * 1024];
    uint8_t* ptr = buf + sizeof(buf);
    RansState state0, state1;
    RansEncInit(&state0);
    RansEncInit(&state1);

    int count = (int)input.size();
    // Odd tail encoded into state0
    if (count & 1) {
        int s = input[count - 1];
        uint32_t freq = syms.freqs[s];
        uint32_t cum = syms.cum_freqs[s];
        RansState x = RansEncRenorm(state0, &ptr, freq, scale_bits);
        uint32_t rem = x % freq;
        state0 = ((x / freq) << scale_bits) + syms.alias_remap[rem + cum];
    }
    // Pairs: (i-1→state1, i-2→state0)
    for (int i = count & ~1; i > 0; i -= 2) {
        int s1 = input[i - 1];
        int s0 = input[i - 2];

        // state1
        { uint32_t freq = syms.freqs[s1];
          uint32_t cum = syms.cum_freqs[s1];
          RansState x = RansEncRenorm(state1, &ptr, freq, scale_bits);
          uint32_t rem = x % freq;
          state1 = ((x / freq) << scale_bits) + syms.alias_remap[rem + cum]; }

        // state0
        { uint32_t freq = syms.freqs[s0];
          uint32_t cum = syms.cum_freqs[s0];
          RansState x = RansEncRenorm(state0, &ptr, freq, scale_bits);
          uint32_t rem = x % freq;
          state0 = ((x / freq) << scale_bits) + syms.alias_remap[rem + cum]; }
    }

    RansEncFlush(&state1, &ptr);
    RansEncFlush(&state0, &ptr);

    size_t comp_size = sizeof(buf) - (ptr - buf);

    printf("{\"op\":\"enc-stream-alias-interleaved2\""
           ",\"scale_bits\":%u"
           ",\"compressed_hex\":\"%s\""
           ",\"compressed_size\":%zu"
           "}\n",
           scale_bits,
           hex_encode(ptr, comp_size).c_str(),
           comp_size);
}

// ---------------------------------------------------------------------------
// Alias interleaved2 decode
// ---------------------------------------------------------------------------
static void trace_dec_stream_alias_interleaved2(uint32_t scale_bits,
                                                const std::vector<uint32_t>& freqs_in,
                                                const std::vector<uint8_t>& compressed,
                                                size_t num_symbols)
{
    uint32_t total = 1u << scale_bits;

    AliasStats syms;
    for (int i = 0; i < ALIAS_NSYMS && i < (int)freqs_in.size(); i++) {
        syms.freqs[i] = freqs_in[i];
    }
    for (int i = (int)freqs_in.size(); i < ALIAS_NSYMS; i++) {
        syms.freqs[i] = 0;
    }
    syms.normalize_freqs(total);
    syms.make_alias_table(scale_bits);

    // Init two interleaved states from the compressed stream
    std::vector<uint8_t> buf(compressed.begin(), compressed.end());
    uint8_t* ptr = buf.data();
    RansState state0, state1;
    RansDecInit(&state0, &ptr);
    RansDecInit(&state1, &ptr);

    size_t n = num_symbols;
    size_t even_n = n & ~(size_t)1;
    std::vector<uint8_t> output(n);

    // Decode pairs: advance both states (step), then renorm both
    for (size_t pos = 0; pos < even_n; pos += 2) {
        uint32_t mask = (1u << scale_bits) - 1;

        // state0 → output[pos]
        { uint32_t xm = state0 & mask;
          uint32_t bucket_id = xm >> (scale_bits - ALIAS_LOG2NSYMS);
          uint32_t bucket2 = bucket_id * 2;
          if (xm < syms.divider[bucket_id]) bucket2++;
          output[pos] = syms.sym_id[bucket2];
          RansDecAdvanceStep(&state0, syms.slot_adjust[bucket2], syms.slot_freqs[bucket2], scale_bits); }

        // state1 → output[pos+1]
        { uint32_t xm = state1 & mask;
          uint32_t bucket_id = xm >> (scale_bits - ALIAS_LOG2NSYMS);
          uint32_t bucket2 = bucket_id * 2;
          if (xm < syms.divider[bucket_id]) bucket2++;
          output[pos + 1] = syms.sym_id[bucket2];
          RansDecAdvanceStep(&state1, syms.slot_adjust[bucket2], syms.slot_freqs[bucket2], scale_bits); }

        RansDecRenorm(&state0, &ptr);
        RansDecRenorm(&state1, &ptr);
    }

    // Odd tail: decode last symbol from state0
    if (even_n < n) {
        uint32_t mask = (1u << scale_bits) - 1;
        uint32_t xm = state0 & mask;
        uint32_t bucket_id = xm >> (scale_bits - ALIAS_LOG2NSYMS);
        uint32_t bucket2 = bucket_id * 2;
        if (xm < syms.divider[bucket_id]) bucket2++;
        output[n - 1] = syms.sym_id[bucket2];
        // Full advance (step + renorm) for tail
        RansDecAdvance(&state0, &ptr, syms.slot_adjust[bucket2], syms.slot_freqs[bucket2], scale_bits);
    }

    printf("{\"op\":\"dec-stream-alias-interleaved2\""
           ",\"scale_bits\":%u"
           ",\"num_symbols\":%zu"
           ",\"decoded_hex\":\"%s\""
           "}\n",
           scale_bits, n,
           hex_encode(output.data(), output.size()).c_str());
}
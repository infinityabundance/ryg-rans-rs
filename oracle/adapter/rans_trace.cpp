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

    printf("{\"op\":\"enc-stream-byte-div\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_size\":%zu"
           ",\"compressed_hex\":\"%s\""
           "}\n",
           scale_bits, input.size(), comp_size, comp_hex.c_str());
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

    printf("{\"op\":\"enc-stream-r64-div\""
           ",\"scale_bits\":%u"
           ",\"input_size\":%zu"
           ",\"compressed_words\":%zu"
           ",\"compressed_bytes\":%zu"
           ",\"compressed_hex\":\"%s\""
           "}\n",
           scale_bits, input.size(), comp_words, comp_bytes, comp_hex.c_str());
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
    } else {
        fprintf(stderr, "Unknown operation: %s\n", op);
        usage(argv[0]);
    }

    return 0;
}

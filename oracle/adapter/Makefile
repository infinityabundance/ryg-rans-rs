# Build the rANS trace adapter (supports 32-bit byte and 64-bit rANS)
CFLAGS  = -O3
LDLIBS  = -lm

rans_trace: rans_trace.cpp platform.h rans_byte.h rans64.h
	g++ -o $@ $< $(CFLAGS) $(LDLIBS)

.PHONY: clean
clean:
	rm -f rans_trace

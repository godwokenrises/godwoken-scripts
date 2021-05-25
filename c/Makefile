TARGET := riscv64-unknown-linux-gnu
CC := $(TARGET)-gcc
LD := $(TARGET)-gcc
OBJCOPY := $(TARGET)-objcopy
SECP256K1_INCLUDES := -I deps/ckb-production-scripts/c/ -I deps/ckb-production-scripts/build/ -I deps/ckb-production-scripts/deps/secp256k1/src/ -I deps/ckb-production-scripts/deps/secp256k1/
CINCLUDES := -I deps/ckb-c-stdlib -I deps/ckb-c-stdlib/libc -I deps/molecule -I . -I build
CFLAGS := $(CINCLUDES) $(SECP256K1_INCLUDES) -DCKB_C_STDLIB_PRINTF -fPIC -O3 -nostdinc -nostdlib -nostartfiles -fvisibility=hidden -Wall -Werror -Wno-nonnull -Wno-nonnull-compare -Wno-unused-function -g
LDFLAGS := -Wl,-static -fdata-sections -ffunction-sections -Wl,--gc-sections
MOLC := moleculec
MOLC_VERSION := 0.6.1
PROTOCOL_VERSION := 074b14268a44e4603e7a6896a2bfdeabe0f71507
PROTOCOL_SCHEMA_URL := https://raw.githubusercontent.com/nervosnetwork/godwoken/${PROTOCOL_VERSION}/crates/types/schemas

# docker pull nervos/ckb-riscv-gnu-toolchain:gnu-bionic-20191012
BUILDER_DOCKER := nervos/ckb-riscv-gnu-toolchain@sha256:aae8a3f79705f67d505d1f1d5ddc694a4fd537ed1c7e9622420a470d59ba2ec3

GENERATORS := build/meta-contract-generator build/sudt-generator build/examples/sum-generator build/examples/account-operation-generator
VALIDATORS := build/meta-contract-validator build/sudt-validator build/examples/sum-validator build/examples/account-operation-validator
SECP256K1_HELPER := deps/ckb-production-scripts/build/secp256k1_data_info.h

LIBS := build/libckb-secp256k1.a

BINS := $(GENERATORS) $(VALIDATORS)

all-via-docker: generate-protocol $(SECP256K1_HELPER)
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} bash -c "make all"

debug-all-via-docker: generate-protocol $(SECP256K1_HELPER)
	docker run --rm -v `pwd`:/code -w /code ${BUILDER_DOCKER} bash -c "make debug-all"

all: $(BINS) $(LIBS)

clean:
	rm -f $(BINS)
	rm -f build/*.debug
	rm -f build/*.h
	rm -f build/*.mol
	rm -f build/*.o
	rm -f build/*.a

debug-all: CFLAGS += -DCKB_C_STDLIB_PRINTF 
debug-all: all

# secp256k1_helper.h
$(SECP256K1_HELPER):
	cd deps/ckb-production-scripts && git submodule init && git submodule update -r && make all-via-docker

build/meta-contract-generator: contracts/meta_contract.c gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/meta-contract-validator: contracts/meta_contract.c gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/sudt-generator: contracts/sudt.c sudt_utils.h gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/sudt-validator: contracts/sudt.c sudt_utils.h gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/sum-generator: examples/sum.c gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/sum-validator: examples/sum.c gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/account-operation-generator: examples/account_operation.c gw_def.h generator_utils.h
	$(CC) $(CFLAGS) -DGW_GENERATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

build/examples/account-operation-validator: examples/account_operation.c gw_def.h validator_utils.h
	$(CC) $(CFLAGS) -DGW_VALIDATOR $(LDFLAGS) -o $@ $<
	$(OBJCOPY) --only-keep-debug $@ $@.debug
	$(OBJCOPY) --strip-debug --strip-all $@

generate-protocol: check-moleculec-version build/blockchain.h build/godwoken.h

fmt:
	clang-format -i -style=Google $(wildcard ./*.h ./*.c)
	git diff --exit-code $(wildcard ./*.h ./*.c)

check-moleculec-version:
	test "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" = ${MOLC_VERSION}

%.mol:
	curl -L -o $@ ${PROTOCOL_SCHEMA_URL}/$@

build/blockchain.h: blockchain.mol
	${MOLC} --language c --schema-file $< > $@

build/godwoken.h: godwoken.mol
	${MOLC} --language c --schema-file $< > $@

install-tools:
	if [ ! -x "$$(command -v "${MOLC}")" ] \
			|| [ "$$(${MOLC} --version | awk '{ print $$2 }' | tr -d ' ')" != "${MOLC_VERSION}" ]; then \
		cargo install --force --version "${MOLC_VERSION}" "${MOLC}"; \
	fi

# build account locks lib

build/libckb-secp256k1.a: build/secp256k1.o
	$(AR) rcs $@ $^

build/secp256k1.o: account_lock_lib/secp256k1.c
	$(CC) $(CFLAGS) -D __SHARED_LIBRARY__ -c -o $@ $<

BIN_DIR = bin
BIN_NAME = rag
OS = $(shell uname -s | tr '[:upper:]' '[:lower:]')
ARCH = $(shell uname -m)

CROSS_TARGETS = \
	aarch64-apple-darwin \
	x86_64-apple-darwin \
	x86_64-unknown-linux-gnu \
	aarch64-unknown-linux-gnu

.PHONY: build release release-all run test lint fmt clean model

build:
	cargo build

release:
	cargo build --release
	mkdir -p $(BIN_DIR)
	cp target/release/rag-lightweight $(BIN_DIR)/$(BIN_NAME)-$(OS)-$(ARCH)

release-all: $(addprefix release-,$(CROSS_TARGETS))

release-%:
	cargo build --release --target $*
	mkdir -p $(BIN_DIR)
	cp target/$*/release/rag-lightweight $(BIN_DIR)/$(BIN_NAME)-$(call target-os,$*)-$(call target-arch,$*)

target-os = $(if $(findstring apple,$1),darwin,$(if $(findstring linux,$1),linux,unknown))
target-arch = $(word 1,$(subst -, ,$1))

run:
	cargo run

test:
	cargo test

lint:
	cargo clippy -- -D warnings
	cargo fmt --check

fmt:
	cargo fmt

clean:
	cargo clean
	rm -rf $(BIN_DIR)

MODEL_DIR = models
MODEL_FILE = $(MODEL_DIR)/embeddinggemma-q8_0.gguf
MODEL_URL = https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF/resolve/main/embeddinggemma-300M-Q8_0.gguf

model: $(MODEL_FILE)
	ollama create embeddinggemma -f Modelfile

$(MODEL_FILE):
	mkdir -p $(MODEL_DIR)
	curl -L -o $(MODEL_FILE) "$(MODEL_URL)"
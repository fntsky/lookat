CHAMBER_DIR ?= ../formicary/tmp/chamber/lookat

.PHONY: build build-frontend build-server copy-to-chamber run check

build: build-frontend build-server

build-frontend:
	npm ci --prefix frontend
	npm run build --prefix frontend

build-server:
	cargo build --release --bin lookat-server
	mkdir -p dist
	cp target/release/lookat-server dist/lookat-server

copy-to-chamber: build
	mkdir -p "$(CHAMBER_DIR)"
	tar --exclude='./.git' --exclude='./target' --exclude='./tmp' --exclude='./frontend/node_modules' --exclude='./data' -cf - . | tar -xf - -C "$(CHAMBER_DIR)"
	mkdir -p "$(CHAMBER_DIR)/data"

run: build
	./start.sh

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
	npm test --prefix frontend
	npm run build --prefix frontend
	python3 tests/receiver_smoke.py

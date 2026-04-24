NATIVE_IMAGE := wasm-bench-native:local
WASM_IMAGE := wasm-bench-wasm:local
NATIVE_CTN := wasm-bench-native
WASM_CTN := wasm-bench-wasm
PORT := 3100
DATA := $(CURDIR)/data

.PHONY: build build-native build-wasm extract bare size size-json startup rps run-native run-wasm stop clean report all

all: build size startup rps bare report

build: build-native build-wasm

extract: build
	mkdir -p bare
	@CID=$$(docker create $(NATIVE_IMAGE)) && \
	 docker cp $$CID:/usr/local/bin/wasm-bench-native bare/wasm-bench-native && \
	 docker rm $$CID >/dev/null
	@CID=$$(docker create $(WASM_IMAGE)) && \
	 docker cp $$CID:/app/bench.wasm bare/bench.wasm && \
	 docker rm $$CID >/dev/null
	@ls -la bare/

bare: extract
	./scripts/measure-bare.sh

build-native:
	docker build -t $(NATIVE_IMAGE) native

build-wasm:
	docker build -t $(WASM_IMAGE) wasm

size:
	@mkdir -p $(DATA)
	@echo "image,bytes,human" > $(DATA)/size.csv
	@NATIVE_BYTES=$$(docker image inspect $(NATIVE_IMAGE) --format '{{.Size}}'); \
	 WASM_BYTES=$$(docker image inspect $(WASM_IMAGE) --format '{{.Size}}'); \
	 NATIVE_H=$$(numfmt --to=iec-i --suffix=B $$NATIVE_BYTES); \
	 WASM_H=$$(numfmt --to=iec-i --suffix=B $$WASM_BYTES); \
	 echo "native,$$NATIVE_BYTES,$$NATIVE_H" >> $(DATA)/size.csv; \
	 echo "wasm,$$WASM_BYTES,$$WASM_H" >> $(DATA)/size.csv; \
	 cat $(DATA)/size.csv | column -t -s,

run-native: stop
	docker run -d --rm --name $(NATIVE_CTN) -p $(PORT):3000 $(NATIVE_IMAGE) >/dev/null

run-wasm: stop
	docker run -d --rm --name $(WASM_CTN) -p $(PORT):3000 $(WASM_IMAGE) >/dev/null

stop:
	-docker rm -f $(NATIVE_CTN) $(WASM_CTN) 2>/dev/null | true

startup: build
	@mkdir -p $(DATA)
	./scripts/measure-startup.sh native $(NATIVE_IMAGE) $(DATA)/startup-native.csv
	./scripts/measure-startup.sh wasm $(WASM_IMAGE) $(DATA)/startup-wasm.csv
	@echo "---"
	@printf "%-10s %10s %10s %10s\n" variant min_ms median_ms max_ms
	@for v in native wasm; do \
	  awk -F, -v v=$$v 'NR>1 {print $$1}' $(DATA)/startup-$$v.csv \
	    | sort -n | awk -v v=$$v 'BEGIN{min=99999999} {a[NR]=$$1; if($$1<min)min=$$1; if($$1>max)max=$$1} END{med=(NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2; printf "%-10s %10d %10d %10d\n", v, min, med, max}'; \
	done

rps: build
	@mkdir -p $(DATA)
	./scripts/measure-rps.sh native $(NATIVE_IMAGE) $(DATA)/rps-native.csv
	./scripts/measure-rps.sh wasm $(WASM_IMAGE) $(DATA)/rps-wasm.csv
	@echo "---"
	@for v in native wasm; do echo "=== $$v ==="; cat $(DATA)/rps-$$v.csv | column -t -s,; done

report:
	@mkdir -p $(DATA)
	@{ \
	  echo "# wasm-bench results"; \
	  echo; \
	  echo "Run: $$(date -u +%Y-%m-%dT%H:%M:%SZ)"; \
	  echo; \
	  echo "## Image size"; \
	  echo; \
	  if [ -f $(DATA)/size.csv ]; then cat $(DATA)/size.csv; fi; \
	  echo; \
	  echo "## Startup"; \
	  echo; \
	  for v in native wasm; do \
	    if [ -f $(DATA)/startup-$$v.csv ]; then \
	      echo "### $$v"; echo; cat $(DATA)/startup-$$v.csv; echo; \
	    fi; \
	  done; \
	  echo; \
	  echo "## RPS"; \
	  echo; \
	  for v in native wasm; do \
	    if [ -f $(DATA)/rps-$$v.csv ]; then \
	      echo "### $$v"; echo; cat $(DATA)/rps-$$v.csv; echo; \
	    fi; \
	  done; \
	} > $(DATA)/summary.md
	@echo "wrote $(DATA)/summary.md"

clean: stop
	-docker rmi $(NATIVE_IMAGE) $(WASM_IMAGE) 2>/dev/null | true
	rm -rf $(DATA)

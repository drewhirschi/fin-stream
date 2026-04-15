COOLIFY_URL := http://gory:8000
APP_UUID := qx36dh9sz8wqauggabhki4h3
COOLIFY_TOKEN := $(shell cat gory_coolify_access_token.txt)
IMAGE := ghcr.io/drewhirschi/fin-stream
BUILDER := fin-stream-multiarch

deploy: ## Force a new deployment
	curl -sS "$(COOLIFY_URL)/api/v1/deploy?uuid=$(APP_UUID)&force=true" \
		-H "Authorization: Bearer $(COOLIFY_TOKEN)" | jq

logs: ## Show recent app logs
	curl -sS "$(COOLIFY_URL)/api/v1/applications/$(APP_UUID)/logs" \
		-H "Authorization: Bearer $(COOLIFY_TOKEN)" | jq

status: ## Show app status
	curl -sS "$(COOLIFY_URL)/api/v1/applications/$(APP_UUID)" \
		-H "Authorization: Bearer $(COOLIFY_TOKEN)" | jq '{name, fqdn, status}'

envs: ## List environment variables
	curl -sS "$(COOLIFY_URL)/api/v1/applications/$(APP_UUID)/envs" \
		-H "Authorization: Bearer $(COOLIFY_TOKEN)" | jq '.[] | {key, value}'

build: ## Build and push multi-arch image to GHCR
	@docker buildx inspect $(BUILDER) >/dev/null 2>&1 || \
		docker buildx create --name $(BUILDER) --use --platform linux/amd64,linux/arm64
	docker buildx use $(BUILDER)
	docker buildx build --platform linux/amd64,linux/arm64 \
		-t $(IMAGE):latest -t $(IMAGE):$(shell git rev-parse --short HEAD) \
		--push .

ship: build deploy ## Build, push, and deploy
	@echo "Shipped and deployed."

APP_CONTAINER := qx36dh9sz8wqauggabhki4h3
DB_CONTAINER := n13nq85wsctjqwd5hcxuilyd

stats: ## Show CPU, memory, and network usage
	@ssh gory "docker stats --no-stream --format 'table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}\t{{.BlockIO}}'" \
		| head -1
	@ssh gory "docker stats --no-stream --format 'table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}\t{{.BlockIO}}'" \
		| grep -E '$(APP_CONTAINER)|$(DB_CONTAINER)'

help: ## Show available commands
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  make %-12s %s\n", $$1, $$2}'

.PHONY: build ship deploy logs status stats envs help

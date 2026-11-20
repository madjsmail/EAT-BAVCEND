CARGO ?= cargo

# Fall back to DATABASE_URL from .env for the clean-db target.
DATABASE_URL ?= $(shell sed -n 's/^DATABASE_URL=//p' .env 2>/dev/null)
export DATABASE_URL

.DEFAULT_GOAL := help
.PHONY: help run watch bootstrap migrate test test-unit test-integration check fmt clippy clean clean-db garage-up garage-down

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

run: ## Start the API server
	$(CARGO) run

watch: ## Restart the API on file changes (requires cargo-watch)
	$(CARGO) watch -x run

bootstrap: ## Create the first organization + admin (ORG, SLUG, EMAIL, PASSWORD, FIRST, LAST)
	$(CARGO) run -- bootstrap --org-name "$(ORG)" --org-slug "$(SLUG)" --email "$(EMAIL)" --password "$(PASSWORD)" --first-name "$(FIRST)" --last-name "$(LAST)"

migrate: ## Apply pending migrations
	sqlx migrate run

test: ## Run all unit and integration tests
	$(CARGO) test

test-unit: ## Run unit tests only
	$(CARGO) test --lib

test-integration: ## Run integration tests only
	$(CARGO) test --tests

check: ## Fast type check
	$(CARGO) check

fmt: ## Format the codebase
	$(CARGO) fmt

clippy: ## Lint with clippy (fails on warnings)
	$(CARGO) clippy --all-targets -- -D warnings

clean: ## Remove build artifacts and logs
	$(CARGO) clean
	rm -rf logs

clean-db: ## Drop the schema and re-seed the demo data (uses DATABASE_URL)
	@test -n "$(DATABASE_URL)" || (echo "DATABASE_URL is not set" && exit 1)
	psql "$(DATABASE_URL)" -v ON_ERROR_STOP=1 -c 'DROP SCHEMA public CASCADE; CREATE SCHEMA public;'
	$(CARGO) run -- bootstrap --org-name "$(ORG)" --org-slug "$(SLUG)" --email "$(EMAIL)" --password "$(PASSWORD)" --first-name "$(FIRST)" --last-name "$(LAST)"

garage-up: ## Start Garage object storage (docker compose)
	docker compose up -d garage

garage-down: ## Stop Garage object storage
	docker compose stop garage

pgweb: ## Open pgweb connected to DATABASE_URL
	pgweb --url "$(DATABASE_URL)"

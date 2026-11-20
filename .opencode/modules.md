# Module Architecture & Layout

## Standard Module Layout (`src/<module>/`)
├── mod.rs              # pub use of public API
├── api/routes.rs       # prefix-free Axum Router
├── dto/                # Request/Response DTOs (validator)
├── entity/             # SQLx row models
├── repository/         # Data access (TenantScopedRepo)
├── service/            # Business logic
└── error.rs            # module-specific error variants extending AppError

# Rust Axum API Boilerplate

Production-ready Rust REST API boilerplate built with **Axum**, **SQLx**, and **PostgreSQL**, designed with clean architecture and scalability in mind.

---

## ✨ Features

- ⚡ **Axum** async web framework
- 🗄️ **PostgreSQL** with **SQLx**
- 🧱 Clean Architecture (handler → service → repository)
- 📦 DTO-based request & response layer
- 🧰 Centralized error handling & standardized response envelope
- 📝 Structured logging with **log4rs**
- 🌱 Environment-based configuration (`.env`)
- 🛑 Graceful shutdown support

---

## 🗂️ Project Structure

```text
src/
├── config/        # Environment & database configuration
├── dto/           # Request & response DTOs
├── entity/        # Database models
├── repository/    # Data access layer (SQLx)
├── service/       # Business logic
├── handler/       # HTTP handlers (Axum)
├── routes/        # Route definitions
├── middleware/    # Custom middleware
├── error/         # Domain & API errors
├── response/      # Standard API responses
├── state/         # Shared application state
└── main.rs        # Application entry point
```

---

## ⚙️ Requirements

* Rust **1.75+**
* PostgreSQL **13+**
* Cargo
* Linux / macOS (Windows via WSL recommended)

---

## 🚀 Getting Started

### 1️⃣ Clone the repository

```bash
git clone https://github.com/ErrLogic/rust-axum-boilerplate
cd rust-axum-boilerplate
```

### 2️⃣ Setup environment variables

Create a `.env` file:

```env
APP_NAME=rust-api
APP_URL=127.0.0.1
APP_PORT=8000

DATABASE_URL=postgres://appuser:change_me_postgres_pass@localhost:5432/appdb
```

### 3️⃣ Run the application

```bash
cargo run
```

Server will start at:

```
http://127.0.0.1:8000
```

---

## 🧪 Response Format

Every endpoint returns a consistent envelope:

**Single object**

```json
{
  "error": false,
  "status": 200,
  "data": {},
  "message": "success"
}
```

**List with pagination**

```json
{
  "error": false,
  "status": 200,
  "data": [],
  "pagination": { "page": 1, "per_page": 20, "total": 0, "total_pages": 0 },
  "message": "success"
}
```

**Error**

```json
{
  "error": true,
  "status": 400,
  "error_stack": [
    { "field": "email", "code": "invalid", "message": "Email is not valid" }
  ],
  "message": "Validation failed"
}
```

Build them with `ApiResponse::success`, `ApiResponse::paginated`, and
`ApiResponse::error` (`src/response/api_response.rs`).

---

## 📝 Logging

This project uses **log4rs** with daily file rotation.

* Console logging
* File logging
* Configurable via `log4rs.yaml`
* Graceful shutdown logging

---

## 🧠 Design Philosophy

This boilerplate is designed to:

* Be **easy to extend**
* Stay **testable & maintainable**
* Separate **business logic from infrastructure**
* Scale from **small services to production workloads**

---

## 📌 Notes

* This project avoids framework magic
* Explicit error handling over panics
* Repository layer is **database-focused only**
* Service layer owns **business rules**

---

## 📄 License

MIT License

---

## 🤝 Contributions

Pull requests and discussions are welcome.
This project is intended as a **learning reference and production starter**.
# Project: anotro-rs

## General Instructions

### 1. Code Standards & Idioms
* **Idiomatic Rust:** Prioritize safe, idiomatic patterns. Use `unsafe` only when strictly necessary, accompanied by a `// SAFETY:` comment.
* **Formatting & Linting:** Run `cargo fmt` before every commit. The codebase must remain `clippy` warning-free (`cargo clippy -- -D warnings`).
* **Error Handling:** Use `Result` and `Option` for robust logic. Avoid `unwrap()` and `panic!` in production code.

### 2. Workflow & Pull Requests
* **Commits:** Use **Conventional Commits** (e.g., `feat:`, `fix:`, `refactor:`).
* **Branches:** Work on descriptive feature branches (e.g., `feature/xyz`) based on `main`.
* **CI Compliance:** PRs are only eligible for merge after passing all automated tests, formatting, and linting checks.

### 3. Testing & Documentation
* **Test Coverage:** Implement unit tests in-file and integration tests in the `tests/` directory. Ensure all doc-tests pass.
* **Public API:** Document all public structs, traits, and functions using `///` comments with clear examples.
* **Transparency:** Comment the reasoning ("why") behind complex logic, not just the "what."

### 4. Dependency Management
* **Minimalism:** Keep the dependency tree lean to reduce compile times and security surface.
* **Security:** Use `cargo audit` regularly to identify and mitigate known vulnerabilities in crates.

### Commit Guidelines

Follow the [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification:

`type(optional-scope): description`

#### Common Types:
* **feat:** A new feature (e.g., adding a new fingerprinting algorithm).
* **fix:** A bug fix.
* **docs:** Documentation only changes.
* **style:** Changes that do not affect the meaning of the code (white-space, formatting, etc).
* **refactor:** A code change that neither fixes a bug nor adds a feature.
* **perf:** A code change that improves performance (e.g., optimizing an iterator).
* **test:** Adding missing tests or correcting existing tests.
* **chore:** Changes to the build process, auxiliary tools, or library updates.

#### Examples:
* `feat(parser): add support for nested annotations`
* `fix(audio): resolve overflow in timestamp calculation`
* `perf: replace Vec allocation with SmallVec in hot path`
* `refactor!: change trait signature for Chromaprint trait` (Note the `!` for Breaking Changes)

## Rules

This section defines the mandatory architectural and coding constraints for **anotro-rs** to ensure maximum performance, long-term maintainability, and optimal interpretability for AI-agentic development.

---

### Architecture and Design Patterns

* **Hexagonal Architecture (Ports and Adapters):** The core business logic—specifically the chromaprint comparison algorithms and timing calculations—must remain isolated and agnostic of external libraries or infrastructure. Interactions with the file system (MKV/MP4) and external tools like FFmpeg must be implemented as **Ports** using the Rust **Trait** system. Concrete implementations or **Adapters** must reside in the outer infrastructure layer to prevent side effects within the immutable core domain logic.
* **Typestate Pattern for Invariant Integrity:** All sequential processing stages, such as audio extraction and fingerprint generation, must be modeled as discrete types. Transition functions must consume the preceding state by value (`self`) rather than using mutable references, ensuring the compiler destroys the previous state once the transformation is complete. This pattern makes invalid states **mathematically unrepresentable**, providing a deterministic guide for stochastic AI code generation.

### Performance and Memory Management

* **Performance-Critical Parallelism:** Data processing pipelines must utilize the `rayon` library to convert sequential iterators into parallel ones for processing multiple episodes simultaneously. Inefficient algorithms, such as frequent calls to `.collect()` that trigger excessive memory allocations, must be avoided in favor of navigating through iterators.
* **Memory Management and Ownership:** Constructor functions should take **owned values** to allow callers to decide when to transfer ownership. Conversely, getter methods must return **references** (`&T`) instead of owned values to prevent unnecessary memory allocations. Shared ownership across threads should be handled via `Arc` (Atomic Reference Counting) rather than cloning large datasets.

### Reliability and Observability

* **Strict Error Handling:** The use of `.unwrap()`, `.expect()`, or the `panic!` macro is strictly prohibited in production-ready code to prevent unexpected crashes. Every fallible operation must return a `Result<T, E>` and utilize the `?` operator for error propagation. Use the `thiserror` crate to define explicit, enumerated domain errors and `anyhow` for high-level application tracebacks.
* **AI-Agentic Readiness and Legibility:** Avoid "macro-heavy" implicit structures that obscure the **Abstract Syntax Tree (AST)**, as hiding logic behind metaprogramming degrades the iterative feedback and reasoning capabilities of LLMs. Developers must use `rustdoc` (`///`) extensively to inject semantic descriptions of parameters and variants directly into the code. This ensures that the documentation acts as a primary instruction matrix for the AI orchestrator's attention window.

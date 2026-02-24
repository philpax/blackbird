## General conventions

### Correctness over convenience

- Model the full error space—no shortcuts or simplified error handling.
- Handle all edge cases, including race conditions, signal timing, and platform differences.
- Use the type system to encode correctness constraints.
- Prefer compile-time guarantees over runtime checks where possible.

### User experience as a primary driver

- Provide structured, helpful error messages that can be rendered with an appropriate library at a later stage.
- Make progress reporting responsive and informative.
- Maintain consistency across platforms even when underlying OS capabilities differ. Use OS-native logic rather than trying to emulate Unix on Windows (or vice versa).
- Write user-facing messages in clear, present tense: "Frobnicator now supports..." not "Frobnicator now supported..."

### Pragmatic incrementalism

- "Not overly generic"—prefer specific, composable logic over abstract frameworks.
- Evolve the design incrementally rather than attempting perfect upfront architecture.

### Production-grade engineering

- Use type system extensively: newtypes, builder patterns, type states, lifetimes.
- Use message passing or the actor model to avoid data races in concurrent code.
- Test comprehensively, including edge cases, race conditions, and stress tests.
- Pay attention to what facilities already exist for testing, and aim to reuse them.
- Getting the details right is really important!

### Documentation

- Use inline comments to explain "why," not just "what".
- Don't add narrative comments in function bodies. Only add a comment if what you're doing is non-obvious or special in some way, or if something needs a deeper "why" explanation.
- Module-level documentation should explain purpose and responsibilities.
- **Always** use periods at the end of code comments.
- **Never** use title case in headings and titles. Always use sentence case.
- Always use the Oxford comma.
- Don't omit articles ("a", "an", "the"). Write "the file has a newer version" not "file has newer version".

## Code style

### Rust edition and linting

- Use Rust 2024 edition.
- Ensure the following checks pass at the end of each complete task (you do not need to do this for intermediate steps):
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo clippy --all-targets --no-default-features -- -D warnings`
  - `cargo test --workspace`
  - `cargo test --workspace --no-default-features`

### Type system patterns

- **Builder patterns** for complex construction (e.g., `TestRunnerBuilder`)
- **Type states** encoded in generics when state transitions matter
- **Lifetimes** used extensively to avoid cloning (e.g., `TestInstance<'a>`)
- **Restricted visibility**: Use `pub(crate)` and `pub(super)` liberally

### Error handling

- Do not use `thiserror`. Instead, manually implement `std::fmt::Error` for a given error `struct` or `enum`.
- Group errors by category with an `ErrorKind` enum when appropriate.
- Provide rich error context using structured error types.
- Two-tier error model:
  - `ExpectedError`: User/external errors with semantic exit codes.
  - Internal errors: Programming errors that may panic or use internal error types.
- Error display messages should be lowercase sentence fragments suitable for "failed to {error}".

### Async patterns

- Do not introduce async to a project without async.
- Use `tokio` for async runtime (multi-threaded).
- Use async for I/O and concurrency, keep other code synchronous.

### Module organization

- Use `mod.rs` files to re-export public items.
- Keep module boundaries strict with restricted visibility.
- Use `#[cfg(unix)]` and `#[cfg(windows)]` for conditional compilation.
- **Always** import types or functions at the very top of the module, with the one exception being `cfg()`-gated functions. Never import types or modules within function contexts, other than this `cfg()`-gated exception.
- It is okay to import enum variants for pattern matching, though.

### Memory and performance

- Use `Arc` or borrows for shared immutable data.
- Use `smol_str` for efficient small string storage.
- Careful attention to cloning referencing. Avoid cloning if code has a natural tree structure.
- Stream data (e.g. iterators) where possible rather than buffering.

## Testing 

### Testing tools

- **test-case**: For parameterized tests.
- **proptest**: For property-based testing.
- **insta**: For snapshot testing.
- **libtest-mimic**: For custom test harnesses.
- **pretty_assertions**: For better assertion output.

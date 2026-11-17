# Rust Project Context & Engineering Standards

## 1. Tech Stack & Toolchain
- **Language**: Rust (2021 / 2024 Edition)
- **Async Runtime**: Tokio (`tokio` with `full` features)
- **Web / RPC Framework**: [按需修改，例如：Axum 0.7 / Tonic]
- **Serialization & DB**: Serde, SQLx (compile-time checked queries)
- **Error Handling**: `thiserror` (用于底层库/领域模型定义精确错误枚举), `anyhow` (用于顶层应用/二进制入口错误传播)
- **Logging & Tracing**: `tracing` + `tracing-subscriber` (禁止在业务代码中直接使用 `println!` 或 `dbg!`)

## 2. Architecture & Design Principles (For @architect)
- **所有权与生命周期优先**：在设计结构体和 Trait 时，优先使用所有权转移或简单借用，仅在确实需要跨线程共享不可变/可变状态时才引入 `Arc<T>` / `Arc<RwLock<T>>`，避免滥用复杂生命周期标注 `'a`。
- **类型驱动设计（Type-Driven Design）**：善用 Newtype 模式（如 `struct UserId(Uuid)`）和枚举状态机在编译期消除非法状态，而不是依赖运行时字符串或整数校验。
- **异步边界清晰**：Trait 中若包含异步方法，优先使用 Rust 原生 `async fn in trait`（RPITIT），若需动态分发（`dyn Trait`）则显式使用 `#[async_trait]`。
- **模块化目录规范**：按业务领域（Domain）划分模块，保持 `lib.rs` 导出清晰的公共 API，`main.rs` 仅负责配置加载与依赖注入。

## 3. Coding & Safety Rules (For @developer)
- **严禁裸用危险解包**：生产业务代码中**严禁使用 `.unwrap()` 和 `.expect()`**（单元测试 `#[cfg(test)]` 除外）。所有潜在失败必须通过 `?` 运算符向上传播，或使用 `ok_or_else`、`unwrap_or_default` 显式处理。
- **零 Unsafe 原则**：严禁编写 `unsafe` 代码块；若底层性能优化确实必须使用，必须先经过架构师评审并附上详细的 `// SAFETY:` 注释。
- **避免无意义的 `.clone()`**：在热路径或大结构体传递中优先传递引用 `&T` 或切片 `&[T]`，不得为了绕过借用检查器而盲目调用 `.clone()`。
- **异步阻塞防护**：在 `async` 上下文中严禁调用 `std::thread::sleep` 或同步文件/网络 I/O，CPU 密集型计算必须使用 `tokio::task::spawn_blocking` 隔离。

## 4. Verification & CI Commands (Mandatory)
每次完成代码编写或重构后，`@developer` 必须按顺序执行以下命令确保全部通过（Zero Warnings）：
1. **代码格式化**：`cargo fmt --all -- --check`
2. **静态严格检查**：`cargo clippy --all-targets --all-features -- -D warnings`
3. **编译与单元测试**：`cargo test --workspace`

## 5. Git Commit Convention
遵循 Conventional Commits 规范，格式为 `<type>(<scope>): <subject>`（例如：`feat(auth): implement jwt token refresh middleware`）。

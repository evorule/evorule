# syntax=docker/dockerfile:1
# evorule-server 多阶段构建
#
# 构建：docker build -t evorule-server .
# 运行：docker run -p 18080:18080 -v $(pwd)/data:/data evorule-server
#
# 环境变量覆盖（优先级高于 CLI 默认值）：
#   EVORULE_ADDR, EVORULE_AUTH_TOKEN, EVORULE_LLM_API_KEY,
#   EVORULE_LLM_BASE_URL, EVORULE_LLM_MODEL, EVORULE_LOG_LEVEL

# ===== 阶段 1：构建 =====
FROM rust:1.82-slim AS builder

# 安装构建依赖（SQLite 开发库 + pkg-config）
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /evorule

# 先复制 Cargo.toml 文件利用 Docker 层缓存
COPY Cargo.toml ./
COPY tier0-tcb/Cargo.toml tier0-tcb/Cargo.toml
COPY tier1-reactor/Cargo.toml tier1-reactor/Cargo.toml
COPY tier2-governance/Cargo.toml tier2-governance/Cargo.toml

# 创建 dummy 源文件以预编译依赖
RUN mkdir -p tier0-tcb/src tier1-reactor/src tier2-governance/src/bin && \
    echo "pub fn _dummy() {}" > tier0-tcb/src/lib.rs && \
    echo "pub fn _dummy() {}" > tier1-reactor/src/lib.rs && \
    echo "pub fn _dummy() {}" > tier2-governance/src/lib.rs && \
    echo "fn main() {}" > tier2-governance/src/bin/evorule_server.rs

# 预编译依赖（利用 Docker 层缓存，后续源码变更不触发重新编译依赖）
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/evorule/target \
    cargo build --release --bin evorule-server || true

# 复制真实源码
COPY tier0-tcb/ tier0-tcb/
COPY tier1-reactor/ tier1-reactor/
COPY tier2-governance/ tier2-governance/

# 构建最终二进制
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/evorule/target \
    cargo build --release --bin evorule-server && \
    cp /evorule/target/release/evorule-server /usr/local/bin/evorule-server

# ===== 阶段 2：运行时 =====
FROM debian:bookworm-slim

# 安装运行时依赖：
# - libsqlite3-0：SQLite 动态库（sqlx 非 bundled 模式）
# - ca-certificates：HTTPS 请求（LLM/HTTP GET）
RUN apt-get update && apt-get install -y --no-install-recommends \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 复制二进制
COPY --from=builder /usr/local/bin/evorule-server /usr/local/bin/evorule-server

# 复制宪法 + 业务规则
COPY tier0-tcb/core_eval.json /etc/evorule/core_eval.json
COPY rules/ /etc/evorule/rules/

# 数据卷（数据库 + memory handler）
VOLUME ["/data"]

EXPOSE 18080

# 默认启动配置
ENV EVORULE_ADDR=0.0.0.0:18080
ENV EVORULE_CORE_EVAL=/etc/evorule/core_eval.json
ENV EVORULE_RULES_DIR=/etc/evorule/rules
ENV EVORULE_DB_PATH=/data/evorule.db
ENV EVORULE_MEMORY_DIR=/data/memory
ENV EVORULE_LOG_LEVEL=info

ENTRYPOINT ["evorule-server"]

// Generated from templates/scaffolds. Update with JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh.
#[cfg(test)]
#[allow(dead_code)]
pub(super) const EMBEDDED_SCAFFOLD_TEMPLATE_FILES_FROM_SNAPSHOT: bool = true;
pub(super) static EMBEDDED_SCAFFOLD_TEMPLATE_FILES: &[EmbeddedScaffoldTemplateFile] = &[
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/astro/astro.config.mjs.jinja", contents: r#"import { defineConfig } from 'astro/config';

export default defineConfig({});
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/astro/package.json.jinja", contents: r#"{
  "name": "<<[ package_name ]>>",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "<<[ install_command ]>> && astro dev --host ${HOST:-127.0.0.1} --port ${PORT:-4321}",
    "build": "astro check && astro build",
    "build:bundle": "astro build",
    "typecheck": "astro check",
    "lint": "astro check",
    "test:coverage": "node -e \"console.log('No coverage required for this Astro app')\"",
    "preview": "astro preview"
  },
  "dependencies": {
    "astro": "^5.0.0"
  },
  "devDependencies": {
    "@astrojs/check": "^0.9.0",
    "typescript": "^5.7.0"
  }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/astro/src/pages/index.astro.jinja", contents: r#"---
const title = "<<[ title ]>>";
---
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width" />
    <title>{title}</title>
  </head>
  <body>
    <main>
      <h1>{title}</h1>
      <p>Marketing site scaffolded by Jig.</p>
    </main>
  </body>
</html>
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/astro/tsconfig.json.jinja", contents: r#"{
  "extends": "astro/tsconfigs/strict"
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/eslint.config.js.jinja", contents: r#"import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "coverage"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
    },
  },
);
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/index.html.jinja", contents: r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title><<[ title ]>></title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/package.json.jinja", contents: r#"{
  "name": "<<[ package_name ]>>",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "<<[ install_command ]>> && vite",
    "build": "tsc -b && vite build",
    "build:bundle": "vite build",
    "typecheck": "tsc -b",
    "lint": "eslint .",
    "test": "vitest run",
    "test:coverage": "vitest run --coverage --coverage.provider=v8 --coverage.reporter=text --coverage.reporter=json-summary",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@vitejs/plugin-react-swc": "^3.7.2",
    "@eslint/js": "^9.0.0",
    "@testing-library/jest-dom": "^6.6.0",
    "@testing-library/react": "^16.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitest/coverage-v8": "^3.0.0",
    "eslint": "^9.0.0",
    "eslint-plugin-react-hooks": "^5.1.0",
    "eslint-plugin-react-refresh": "^0.4.16",
    "globals": "^15.0.0",
    "jsdom": "^25.0.0",
    "typescript": "^5.7.0",
    "typescript-eslint": "^8.0.0",
    "vite": "^6.0.0",
    "vitest": "^3.0.0"
  }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/src/App.test.tsx.jinja", contents: r#"import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { App } from "./App";

it("renders the app shell", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "<<[ title ]>>" })).toBeInTheDocument();
});
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/src/App.tsx.jinja", contents: r#"export function App() {
  return (
    <main>
      <h1><<[ title ]>></h1>
      <p><<[ subtitle ]>></p>
    </main>
  );
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/src/main.tsx.jinja", contents: r#"import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/src/test-setup.ts.jinja", contents: r#"import "@testing-library/jest-dom/vitest";
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/tsconfig.json.jinja", contents: r#"{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "types": ["vitest/globals"]
  },
  "include": ["src"]
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/frontend/vite-react/vite.config.ts.jinja", contents: r#"/// <reference types="vitest" />
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

const devPort = Number(process.env.PORT);
const server =
  Number.isInteger(devPort) && devPort > 0
    ? {
        host: "127.0.0.1",
        hmr: {
          host: "127.0.0.1",
          clientPort: devPort,
        },
      }
    : {};
const apiOrigin =
  process.env.API_ORIGIN ??
  process.env.JIG_DEV_API_ORIGIN ??
  "http://api.<<[ repo_name ]>>.localhost:1355";

export default defineConfig({
  server: {
    ...server,
    proxy: apiOrigin
      ? {
          "/api": {
            target: apiOrigin,
            changeOrigin: true,
          },
          "/health": {
            target: apiOrigin,
            changeOrigin: true,
          },
        }
      : undefined,
  },
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/main.tsx"],
    },
  },
});
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/Cargo.toml.jinja", contents: r#"[workspace]
resolver = "2"
members = [
  "apps/<<[ package_name ]>>-api",
  "crates/<<[ package_name ]>>-core",
  "crates/<<[ package_name ]>>",
  "crates/<<[ package_name ]>>-http",
  "crates/<<[ package_name ]>>-test-support",
[% if db_enabled %]
  "crates/<<[ package_name ]>>-db",
[% endif %]
]

[workspace.package]
edition = "2024"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
anyhow = "1"
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "signal", "time"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors", "request-id"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
[% if db_enabled %]
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "<<[ sqlx_driver ]>>", "migrate", "macros"] }
[% endif %]
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/apps/api/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-api"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
anyhow.workspace = true
axum.workspace = true
<<[ package_name ]>> = { path = "../../crates/<<[ package_name ]>>"[% if db_enabled %], features = ["db"][% endif %] }
<<[ package_name ]>>-http = { path = "../../crates/<<[ package_name ]>>-http"[% if db_enabled %], features = ["db"][% endif %] }
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/apps/api/src/main.rs.jinja", contents: r#"use std::{net::SocketAddr, process::ExitCode};

use anyhow::Context;
use <<[ module_name ]>>::AppConfig;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    install_panic_hook();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = ?error, "API server failed");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let config = AppConfig::from_env().context("Failed to load application config")?;
    let addr: SocketAddr = config.bind_addr();
    let state = <<[ module_name ]>>::AppState::from_config(config)
        .await
        .context("Failed to initialize application state")?;

    serve(addr, state).await
}

async fn serve(addr: SocketAddr, state: <<[ module_name ]>>::AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind API listener to {addr}"))?;
    let bound_addr = listener
        .local_addr()
        .context("Failed to read API listener address after bind")?;
    tracing::info!(%bound_addr, "listening");

    axum::serve(listener, <<[ module_name ]>>_http::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("API server exited with an error")?;
    Ok(())
}

fn init_tracing() {
    let default_filter = "<<[ module_name ]>>=info,tower_http=info";
    tracing_subscriber::fmt()
        .with_env_filter(
            // If you rename the application crate, update this default filter too.
            std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.into()),
        )
        .init();
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        tracing::error!(panic = %panic_info, "application panic");
        default_hook(panic_info);
    }));
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::error!(error = %error, "failed to listen for SIGTERM");
                    wait_for_ctrl_c().await;
                    return;
                }
            };

        tokio::select! {
            result = tokio::signal::ctrl_c() => log_ctrl_c_result(result),
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    wait_for_ctrl_c().await;
}

async fn wait_for_ctrl_c() {
    log_ctrl_c_result(tokio::signal::ctrl_c().await);
}

fn log_ctrl_c_result(result: std::io::Result<()>) {
    if let Err(error) = result {
        tracing::error!(error = %error, "failed to listen for Ctrl-C");
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/app/AGENTS.md.jinja", contents: r#"# <<[ package_name ]>>

## Purpose

Owns application configuration, shared application state, and business use cases for `<<[ repo_name ]>>`.

## Key entrypoints

- `src/lib.rs`: `AppConfig` parses environment settings and `AppState` carries typed runtime state.

## Edit here for X

- Add app-owned configuration fields, validation, and defaults in `AppConfig`.
- Add business use cases and state accessors here before exposing them through HTTP handlers.
- Keep transport DTOs, Axum extractors, and route wiring in `crates/<<[ package_name ]>>-http`.

## Invariants

- Parse environment configuration once at startup, then pass typed config into `AppState`.
- Do not read request transport details in this crate.
[% if db_enabled %]
- Access persistence through the `<<[ package_name ]>>-db` boundary and keep DB wiring behind the `db` feature.
[% endif %]

## Common commands

- `cargo test -p <<[ package_name ]>>`
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/app/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
<<[ package_name ]>>-core = { path = "../<<[ package_name ]>>-core" }
[% if db_enabled %]
<<[ package_name ]>>-db = { path = "../<<[ package_name ]>>-db", optional = true }
[% endif %]
anyhow.workspace = true
serde.workspace = true
thiserror.workspace = true
tracing.workspace = true

[features]
default = []
[% if db_enabled %]
db = ["dep:<<[ package_name ]>>-db"]
[% endif %]
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/app/src/lib.rs.jinja", contents: r#"use std::net::SocketAddr;

use anyhow::{Context, Result};
use <<[ module_name ]>>_core::AppVersion;
[% if db_enabled %]
#[cfg(feature = "db")]
pub use <<[ module_name ]>>_db as db;
[% endif %]

#[derive(Clone, Debug)]
pub struct AppConfig {
    version: AppVersion,
    bind_addr: SocketAddr,
[% if db_enabled %]
    #[cfg(feature = "db")]
    database_url: Option<String>,
[% endif %]
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_version(env!("CARGO_PKG_VERSION"))
    }

    pub fn from_env_with_version(version: impl Into<String>) -> Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:0".into());
        let bind_addr = bind_addr.parse().with_context(|| {
            format!("Failed to parse BIND_ADDR '{bind_addr}' as a socket address")
        })?;
[% if db_enabled %]

        #[cfg(feature = "db")]
        let database_url = Some(
            std::env::var("DATABASE_URL")
                .context("DATABASE_URL is required when the db feature is enabled")?,
        );
[% endif %]

        Ok(Self {
            version: AppVersion::new(version),
            bind_addr,
            [% if db_enabled %]
            #[cfg(feature = "db")]
            database_url,
            [% endif %]
        })
    }

    pub fn for_tests() -> Self {
        Self::for_tests_with_version(env!("CARGO_PKG_VERSION"))
    }

    pub fn for_tests_with_version(version: impl Into<String>) -> Self {
        Self {
            version: AppVersion::new(version),
            bind_addr: "127.0.0.1:0".parse().expect("test bind address is valid"),
            [% if db_enabled %]
            #[cfg(feature = "db")]
            database_url: None,
            [% endif %]
        }
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub fn version(&self) -> &AppVersion {
        &self.version
    }
[% if db_enabled %]

    #[cfg(feature = "db")]
    pub fn database_url(&self) -> Option<&str> {
        self.database_url.as_deref()
    }
[% endif %]
}

#[derive(Clone, Debug)]
pub struct AppState {
    config: AppConfig,
    [% if db_enabled %]
    #[cfg(feature = "db")]
    db: Option<db::Db>,
    [% endif %]
}

impl AppState {
    pub fn new() -> Self {
        Self::for_tests()
    }

    pub fn new_with_version(version: impl Into<String>) -> Self {
        Self::for_tests_with_version(version)
    }

    pub fn for_tests() -> Self {
        Self::for_tests_with_version(env!("CARGO_PKG_VERSION"))
    }

    pub fn for_tests_with_version(version: impl Into<String>) -> Self {
        Self::from_test_config(AppConfig::for_tests_with_version(version))
    }

    pub fn from_test_config(config: AppConfig) -> Self {
        [% if db_enabled %]
        Self {
            config,
            #[cfg(feature = "db")]
            db: None,
        }
        [% else %]
        Self { config }
        [% endif %]
    }

    pub async fn from_config(config: AppConfig) -> Result<Self> {
        [% if db_enabled %]
        #[cfg(feature = "db")]
        {
            let database_url = config.database_url().ok_or_else(|| {
                anyhow::anyhow!("DATABASE_URL is required to initialize the database")
            })?;
            let db = db::Db::connect(database_url)
                .await
                .context("Failed to connect to the database")?;
            db.migrate()
                .await
                .context("Failed to run database migrations")?;
            return Ok(Self {
                config,
                db: Some(db),
            });
        }

        #[cfg(not(feature = "db"))]
        {
            Ok(Self {
                config,
                #[cfg(feature = "db")]
                db: None,
            })
        }
        [% else %]
        Ok(Self { config })
        [% endif %]
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn version(&self) -> &AppVersion {
        self.config.version()
    }

    pub fn is_ready(&self) -> bool {
        [% if db_enabled %]
        #[cfg(feature = "db")]
        {
            return self.db.is_some();
        }

        #[cfg(not(feature = "db"))]
        {
            true
        }
        [% else %]
        true
        [% endif %]
    }
[% if db_enabled %]

    #[cfg(feature = "db")]
    pub fn db(&self) -> Option<&db::Db> {
        self.db.as_ref()
    }
[% endif %]
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use <<[ module_name ]>>_core::APP_NAME;

    #[test]
    fn app_state_uses_current_version() {
        assert_eq!(AppState::new().version().name, APP_NAME);
    }

    #[test]
    fn test_config_uses_loopback_bind_addr() {
        assert_eq!(
            AppConfig::for_tests().bind_addr().to_string(),
            "127.0.0.1:0"
        );
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/core/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-core"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/core/src/lib.rs.jinja", contents: r#"use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "<<[ repo_name ]>>";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppVersion {
    pub name: String,
    pub version: String,
}

impl AppVersion {
    pub fn current() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"))
    }

    pub fn new(version: impl Into<String>) -> Self {
        Self {
            name: APP_NAME.into(),
            version: version.into(),
        }
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/db/AGENTS.md.jinja", contents: r#"# <<[ package_name ]>>-db

## Purpose

Owns persistence for `<<[ repo_name ]>>`, including database pools, migrations, repositories, and persistence DTOs.

## Key entrypoints

- `src/lib.rs`: database connection, migration helper, and shared DB handle.
- `../../migrations`: SQLx migrations generated for this application.

## Edit here for X

- Add SQLx queries, transaction helpers, repository functions, and persistence mappers here.
- Keep HTTP DTOs in `crates/<<[ package_name ]>>-http` and business decisions in `crates/<<[ package_name ]>>`.

## Invariants

- Run migrations through the DB boundary before serving readiness.
- Do not expose raw request data structures from this crate.
- Keep database access feature-gated from the app crate through the `db` feature.

## Common commands

- `cargo test -p <<[ package_name ]>>-db`
- `sqlx migrate run`
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/db/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-db"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
anyhow.workspace = true
sqlx.workspace = true
tokio.workspace = true
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/db/src/lib.rs.jinja", contents: r#"use std::time::Duration;

use anyhow::Context;

pub type DbPool = sqlx::<<[ db_pool ]>>;

pub const DEFAULT_DB_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Db {
    pool: DbPool,
}

impl Db {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        Self::connect_with_timeout(database_url, DEFAULT_DB_TIMEOUT).await
    }

    pub async fn connect_with_timeout(
        database_url: &str,
        timeout_after: Duration,
    ) -> anyhow::Result<Self> {
        let pool = tokio::time::timeout(timeout_after, DbPool::connect(database_url))
            .await
            .with_context(|| {
                format!("Timed out connecting to database after {timeout_after:?}")
            })??;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        self.migrate_with_timeout(DEFAULT_DB_TIMEOUT).await
    }

    pub async fn migrate_with_timeout(&self, timeout_after: Duration) -> anyhow::Result<()> {
        tokio::time::timeout(
            timeout_after,
            sqlx::migrate!("<<[ migration_path ]>>").run(&self.pool),
        )
        .await
        .with_context(|| {
            format!("Timed out running database migrations after {timeout_after:?}")
        })??;
        Ok(())
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/http/AGENTS.md.jinja", contents: r#"# <<[ package_name ]>>-http

## Purpose

Owns the Axum and HTTP boundary for `<<[ repo_name ]>>`: routes, handlers, middleware, extractors, and HTTP DTOs.

## Key entrypoints

- `src/lib.rs`: builds the public router, request middleware, health endpoints, and API handlers.

## Edit here for X

- Add or change routes, handler functions, HTTP response codes, headers, request IDs, tracing middleware, and extractors here.
- Call app-crate use cases through `AppState` instead of placing business logic in handlers.

## Invariants

- Keep handlers thin and delegate domain work to `<<[ package_name ]>>`.
- Keep observability middleware at this HTTP boundary.
- Preserve `/health/live` for process liveness and `/health/ready` for dependency readiness.

## Common commands

- `cargo test -p <<[ package_name ]>>-http`
- `cargo test -p <<[ package_name ]>>-test-support --test http`
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/http/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-http"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
<<[ package_name ]>> = { path = "../<<[ package_name ]>>"[% if db_enabled %], features = ["db"][% endif %] }
<<[ package_name ]>>-core = { path = "../<<[ package_name ]>>-core" }
axum.workspace = true
tower-http.workspace = true
tracing.workspace = true

[features]
default = []
[% if db_enabled %]
db = ["<<[ package_name ]>>/db"]
[% endif %]
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/http/src/lib.rs.jinja", contents: r#"use axum::{
    Json, Router,
    extract::State,
    http::{HeaderName, StatusCode},
    routing::get,
};
use <<[ module_name ]>>::AppState;
use <<[ module_name ]>>_core::AppVersion;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(live))
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/api/version", get(version))
        .with_state(state)
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
}

async fn live() -> &'static str {
    "ok"
}

async fn ready(State(state): State<AppState>) -> StatusCode {
    if state.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn version(State(state): State<AppState>) -> Json<AppVersion> {
    Json(state.version().clone())
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/AGENTS.md.jinja", contents: r#"# <<[ package_name ]>>-test-support

## Purpose

Owns reusable test harness code only. It should make integration tests concise without becoming production code.

## Key entrypoints

- `src/app.rs`: in-memory Axum test app.
- `src/http.rs`: HTTP request helpers.
- `src/responses.rs`: response assertions and JSON decoding.
[% if db_enabled %]
- `src/db.rs`: database fixture helpers.
[% endif %]
- `tests/http.rs`: scaffold smoke tests for the HTTP boundary.

## Edit here for X

- Add fixtures, scenario builders, response assertions, and integration-test utilities here.
- Keep production handlers in `crates/<<[ package_name ]>>-http` and use cases in `crates/<<[ package_name ]>>`.

## Invariants

- Do not add production-only code paths to this crate.
- Prefer behavior assertions over implementation-detail assertions.
- Keep test state construction explicit so DB-backed readiness cannot accidentally pass without a DB fixture.

## Common commands

- `cargo test -p <<[ package_name ]>>-test-support`
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-test-support"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
<<[ package_name ]>> = { path = "../<<[ package_name ]>>"[% if db_enabled %], features = ["db"][% endif %] }
<<[ package_name ]>>-core = { path = "../<<[ package_name ]>>-core" }
<<[ package_name ]>>-http = { path = "../<<[ package_name ]>>-http"[% if db_enabled %], features = ["db"][% endif %] }
[% if db_enabled %]
<<[ package_name ]>>-db = { path = "../<<[ package_name ]>>-db" }
anyhow.workspace = true
sqlx.workspace = true
[% endif %]
axum.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tower = { workspace = true, features = ["util"] }
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/app.rs.jinja", contents: r#"use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use crate::responses::TestResponse;

const TEST_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct TestApp {
    router: Router,
}

impl TestApp {
    pub fn new() -> Self {
        Self::from_state(<<[ module_name ]>>::AppState::for_tests())
    }

    pub fn from_state(state: <<[ module_name ]>>::AppState) -> Self {
        Self::from_router(<<[ module_name ]>>_http::router(state))
    }

    pub fn from_router(router: Router) -> Self {
        Self { router }
    }

    pub async fn request(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("test router returned an error");
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, TEST_BODY_LIMIT)
            .await
            .expect("failed to read response body");
        TestResponse::new(parts.status, parts.headers, body)
    }

    pub async fn request_with_headers(
        &self,
        method: Method,
        uri: &str,
        body: Option<Body>,
        headers: HeaderMap,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);
        let request_headers = builder.headers_mut().expect("request builder is valid");
        request_headers.extend(headers);
        self.request(
            builder
                .body(body.unwrap_or_else(Body::empty))
                .expect("test request is valid"),
        )
        .await
    }

    pub async fn get(&self, uri: &str) -> TestResponse {
        self.request(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .expect("test request is valid"),
        )
        .await
    }

    pub async fn get_json<T>(&self, uri: &str) -> T
    where
        T: DeserializeOwned,
    {
        self.get(uri).await.assert_status(StatusCode::OK).json()
    }

    pub async fn post_json<T, B>(&self, uri: &str, body: &B) -> T
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.try_post_json(uri, body)
            .await
            .assert_status(StatusCode::OK)
            .json()
    }

    pub async fn try_post_json<B>(&self, uri: &str, body: &B) -> TestResponse
    where
        B: Serialize + ?Sized,
    {
        let json = serde_json::to_vec(body).expect("test JSON body serializes");
        self.request(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .expect("test request is valid"),
        )
        .await
    }
}

impl Default for TestApp {
    fn default() -> Self {
        Self::new()
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/db.rs.jinja", contents: r#"use anyhow::{Context, bail};

pub type TestDbPool = <<[ module_name ]>>_db::DbPool;

#[derive(Clone, Debug)]
pub struct DatabaseTestConfig {
    database_url: String,
}

impl DatabaseTestConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .context("set TEST_DATABASE_URL or DATABASE_URL for database-backed tests")?;
        Ok(Self { database_url })
    }

    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
        }
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

pub struct TestDatabase {
    db: <<[ module_name ]>>_db::Db,
}

impl TestDatabase {
    pub async fn connect(config: &DatabaseTestConfig) -> anyhow::Result<Self> {
        let db = <<[ module_name ]>>_db::Db::connect(config.database_url()).await?;
        Ok(Self { db })
    }

    pub fn pool(&self) -> &TestDbPool {
        self.db.pool()
    }
}

pub fn validate_test_database_name(name: &str) -> anyhow::Result<()> {
    if !name.starts_with("test_db_") {
        bail!("test database names must start with test_db_");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("test database names may only contain lowercase ASCII letters, digits, and _");
    }
    Ok(())
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/http.rs.jinja", contents: r#"use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, header},
};
use serde::Serialize;

pub fn request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .expect("test request is valid")
}

pub fn json_request<B>(method: Method, uri: &str, body: &B) -> Request<Body>
where
    B: Serialize + ?Sized,
{
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(body).expect("test JSON body serializes"),
        ))
        .expect("test request is valid")
}

pub fn headers(values: impl IntoIterator<Item = (&'static str, &'static str)>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in values {
        headers.insert(name, value.parse().expect("test header value is valid"));
    }
    headers
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/lib.rs.jinja", contents: r#"pub mod app;
[% if db_enabled %]
pub mod db;
[% endif %]
pub mod http;
pub mod responses;

pub use app::TestApp;
pub use responses::TestResponse;

pub fn test_router() -> axum::Router {
    <<[ module_name ]>>_http::router(<<[ module_name ]>>::AppState::new())
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/responses.rs.jinja", contents: r#"use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use serde::de::DeserializeOwned;

#[derive(Clone, Debug)]
pub struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl TestResponse {
    pub fn new(status: StatusCode, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json<T>(&self) -> T
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "failed to decode response JSON: {error}\nstatus: {}\nbody:\n{}",
                self.status,
                self.text()
            )
        })
    }

    pub fn assert_status(self, expected: StatusCode) -> Self {
        assert_eq!(
            self.status,
            expected,
            "unexpected response status\nbody:\n{}",
            self.text()
        );
        self
    }

    pub fn assert_error(self, expected_status: StatusCode, expected_code: &str) -> Self {
        self.assert_status(expected_status)
            .assert_json_field("code", expected_code)
    }

    pub fn assert_json_field(self, field: &str, expected: &str) -> Self {
        let value: serde_json::Value = self.json();
        assert_eq!(
            value.get(field).and_then(serde_json::Value::as_str),
            Some(expected),
            "unexpected JSON field '{field}' in response: {value}"
        );
        self
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/tests/http.rs.jinja", contents: r#"use axum::http::StatusCode;
use <<[ module_name ]>>_core::{APP_NAME, AppVersion};
use <<[ module_name ]>>_test_support::TestApp;

#[tokio::test]
async fn health_returns_ok() {
    let app = TestApp::new();

    let response = app.get("/health/live").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "ok");
}

#[tokio::test]
async fn readiness_reflects_state() {
    let app = TestApp::new();

    let response = app.get("/health/ready").await;

    [% if db_enabled %]
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    [% else %]
    assert_eq!(response.status(), StatusCode::OK);
    [% endif %]
}

#[tokio::test]
async fn responses_include_request_id() {
    let app = TestApp::new();

    let response = app.get("/health/live").await;

    assert!(response.headers().contains_key("x-request-id"));
}

#[tokio::test]
async fn version_returns_json() {
    let app = TestApp::new();

    let version: AppVersion = app.get_json("/api/version").await;

    assert_eq!(version.name, APP_NAME);
}
"# },
];

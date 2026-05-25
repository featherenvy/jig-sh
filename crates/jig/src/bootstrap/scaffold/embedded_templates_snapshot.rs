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
tower-http = { version = "0.6", features = ["trace", "cors"] }
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
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/apps/api/src/main.rs.jinja", contents: r#"use std::net::SocketAddr;

use anyhow::Context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            // If you rename the application crate, update this default filter too.
            std::env::var("RUST_LOG").unwrap_or_else(|_| "<<[ module_name ]>>=info,tower_http=info".into()),
        )
        .init();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:0".into());
    let addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("Failed to parse BIND_ADDR '{bind_addr}' as a socket address"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind API listener to {addr}"))?;
    let bound_addr = listener
        .local_addr()
        .context("Failed to read API listener address after bind")?;
    tracing::info!(%bound_addr, "listening");

    axum::serve(
        listener,
        <<[ module_name ]>>::router(<<[ module_name ]>>::AppState::new_with_version(env!("CARGO_PKG_VERSION"))),
    )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("API server exited with an error")?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
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
axum.workspace = true
serde.workspace = true
thiserror.workspace = true
tower-http.workspace = true
tracing.workspace = true

[features]
default = []
[% if db_enabled %]
db = ["dep:<<[ package_name ]>>-db"]
[% endif %]
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/app/src/lib.rs.jinja", contents: r#"use axum::{Json, Router, extract::State, routing::get};
use <<[ module_name ]>>_core::AppVersion;
[% if db_enabled %]
#[cfg(feature = "db")]
pub use <<[ module_name ]>>_db as db;
[% endif %]

#[derive(Clone, Debug)]
pub struct AppState {
    version: AppVersion,
}

impl AppState {
    pub fn new() -> Self {
        Self::new_with_version(env!("CARGO_PKG_VERSION"))
    }

    pub fn new_with_version(version: impl Into<String>) -> Self {
        Self {
            version: AppVersion::new(version),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/version", get(version))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn version(State(state): State<AppState>) -> Json<AppVersion> {
    Json(state.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_uses_current_version() {
        assert_eq!(AppState::new().version.name, <<[ module_name ]>>_core::APP_NAME);
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

#[derive(Clone)]
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
            .with_context(|| format!("Timed out connecting to database after {timeout_after:?}"))??;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        self.migrate_with_timeout(DEFAULT_DB_TIMEOUT).await
    }

    pub async fn migrate_with_timeout(&self, timeout_after: Duration) -> anyhow::Result<()> {
        tokio::time::timeout(timeout_after, sqlx::migrate!("<<[ migration_path ]>>").run(&self.pool))
            .await
            .with_context(|| format!("Timed out running database migrations after {timeout_after:?}"))??;
        Ok(())
    }
}
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/Cargo.toml.jinja", contents: r#"[package]
name = "<<[ package_name ]>>-test-support"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
<<[ package_name ]>> = { path = "../<<[ package_name ]>>" }
axum.workspace = true
"# },
    EmbeddedScaffoldTemplateFile { relative_path: "rust-react/workspace/crates/test-support/src/lib.rs.jinja", contents: r#"pub fn test_router() -> axum::Router {
    <<[ module_name ]>>::router(<<[ module_name ]>>::AppState::new())
}
"# },
];

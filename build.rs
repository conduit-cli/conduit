//! Build script for conduit.
//!
//! When the `web` feature is enabled, this script compiles the React frontend
//! before embedding it into the binary with rust-embed.

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run build if frontend source changes
    println!("cargo::rerun-if-changed=web/src");
    println!("cargo::rerun-if-changed=web/package.json");
    println!("cargo::rerun-if-changed=web/vite.config.ts");
    println!("cargo::rerun-if-changed=web/tailwind.config.js");
    println!("cargo::rerun-if-changed=web/index.html");

    // Only build frontend when the web feature is enabled
    if env::var("CARGO_FEATURE_WEB").is_err() {
        return;
    }

    let web_dir = Path::new("web");

    // Check if web directory exists
    if !web_dir.exists() {
        println!("cargo::warning=web/ directory not found, skipping frontend build");
        return;
    }

    // Check if node_modules exists, if not run npm install
    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo::warning=Installing frontend dependencies...");
        let status = Command::new("npm")
            .arg("install")
            .current_dir(web_dir)
            .status()
            .expect("Failed to run npm install");

        if !status.success() {
            panic!("npm install failed");
        }
    }

    // Build the frontend
    println!("cargo::warning=Building frontend...");
    let status = Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir(web_dir)
        .status()
        .expect("Failed to run npm build");

    if !status.success() {
        panic!("Frontend build failed");
    }

    // Verify dist directory was created
    let dist_dir = web_dir.join("dist");
    if !dist_dir.exists() {
        panic!("Frontend build did not produce dist/ directory");
    }

    println!("cargo::warning=Frontend build complete!");
}
